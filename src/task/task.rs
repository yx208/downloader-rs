use std::future::Future;
use std::num::{NonZeroU8, NonZeroUsize};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use tokio_util::sync::CancellationToken;
use url::Url;
use anyhow::Result;
use crate::downloader::chunk_info::ChunkInfo;
use crate::downloader::chunk_range::ChunkRange;
use crate::downloader::download_way::DownloadWay;
use crate::downloader::downloader::{DownloaderConfig, FileDownloader};
use crate::downloader::error::{DownloadEndCause, DownloadError, DownloadStartError};
use crate::task::archive_data::DownloadArchiveData;

pub struct TaskOptions {
    url: Url,
    file_size: u64,
    save_dir: PathBuf,
    filename: String
}

pub struct DownloadTask {
    downloader: FileDownloader,
    status: TaskStatus,
    cancel_token: CancellationToken,
}

impl DownloadTask {
    pub fn new(options: TaskOptions) -> DownloadTask {
        // 为了保证任务之间独立，token 必须由每个 task 自己生成
        let cancel_token = CancellationToken::new();
        let config = DownloaderConfig {
            url: options.url,
            file_name: options.filename,
            save_dir: options.save_dir,
            chunk_size: NonZeroUsize::new(1024 * 1024 * 4).unwrap(),
            retry_count: 3,
            connection_count: NonZeroU8::new(3).unwrap(),
            cancel_token: Some(cancel_token.clone()),
        };

        Self {
            cancel_token,
            status: TaskStatus::Pending,
            downloader: FileDownloader::new(options.file_size, config)
        }
    }

    pub async fn run(&mut self) -> Result<impl Future<Output=Result<DownloadEndCause, DownloadError>>, DownloadStartError> {
        self.downloader.download()
    }

    pub async fn cancel(&self) {
        self.downloader.cancel().await;
    }

    pub async fn archive(&self) -> Option<DownloadArchiveData> {
        let file_size = self.downloader.file_size;

        if let Some(downloading_state) = self.downloader.downloading_state.read().clone() {
            match &downloading_state.download_way {
                DownloadWay::Range(chunk_manger) => {
                    let iter_data = {
                        let data = chunk_manger.chunk_iter.data.read();
                        data.clone()
                    };
                    let downloading_chunks: Vec<_> = chunk_manger.get_downloading_chunks().await;
                    let last_incomplete_chunks: Vec<_> = downloading_chunks
                        .iter()
                        .filter_map(|chunk| {
                            let downloaded = chunk.downloaded.load(Ordering::SeqCst);
                            // 这个 chunk 已经下完
                            if downloaded == chunk.chunk_info.range.len() {
                                None
                            } else {
                                let start = chunk.chunk_info.range.start + downloaded;
                                let end = chunk.chunk_info.range.end;

                                Some(ChunkInfo {
                                    index: chunk.chunk_info.index,
                                    range: ChunkRange::new(start, end)
                                })
                            }
                        })
                        .collect();

                    Some(DownloadArchiveData {
                        downloaded_len: file_size - iter_data.remaining_len(),
                        remaining_chunks: iter_data.remaining,
                        last_incomplete_chunks,
                    })
                }
                // 暂时没有 Single
                DownloadWay::Single(_) => None
            }
        } else {
            None
        }
    }
}

pub enum TaskStatus {
    Pending,
    Running,
    Stopped,
    Paused,
}

mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::io::AsyncWriteExt;
    use tokio::sync::Mutex;
    use url::Url;
    use crate::downloader::error::DownloadEndCause;
    use crate::downloader::util::get_file_length;
    use crate::task::task::{DownloadTask, TaskOptions};

    async fn create_task() -> DownloadTask {
        let file_url = Url::parse("https://tasset.xgy.tv/down/resources/agency/CeShiJiGou_1/QianDuanBuMen_2/dc9152119c160a601e5f684795fb4ea2_16/20241118/18154964fcaafce36522519013.mp4").unwrap();
        let file_size = get_file_length(file_url.clone())
            .await
            .unwrap();
        let options = TaskOptions {
            file_size,
            url: file_url,
            save_dir: PathBuf::from("C:/Users/User/Downloads"),
            filename: "demo.mp4".to_string(),
        };

        DownloadTask::new(options)
    }

    #[tokio::test]
    async fn should_be_resume() -> anyhow::Result<()> {
        let download_task = Arc::new(Mutex::new(create_task().await));

        let download_task_clone = download_task.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            download_task_clone.lock().await.cancel().await;
        });

        let download_task_clone = download_task.clone();
        let run = async move {
            let result = {
                let mut task = download_task_clone.lock().await;
                task.run().await
            };

            let end_cause = result.unwrap().await.unwrap();
            match end_cause {
                DownloadEndCause::Finished => {
                    println!("Finished");
                }
                DownloadEndCause::Canceled => {}
            };
        };

        run.await;

        let download_task_clone = download_task.clone();
        let run = async move {
            let result = {
                let mut task = download_task_clone.lock().await;
                task.run().await
            };

            let end_cause = result.unwrap().await.unwrap();
            match end_cause {
                DownloadEndCause::Finished => {
                    println!("Finished");
                }
                DownloadEndCause::Canceled => {}
            };
        };

        run.await;

        Ok(())
    }

    #[tokio::test]
    async fn should_be_cancel() -> anyhow::Result<()> {
        let file_url = Url::parse("http://localhost:23333/image.png")?;
        let file_size = get_file_length(file_url.clone())
            .await
            .unwrap();
        let options = TaskOptions {
            file_size,
            url: file_url,
            save_dir: PathBuf::from("C:/Users/User/Downloads"),
            filename: "demo.mkv".to_string(),
        };
        let download_task = Arc::new(Mutex::new(DownloadTask::new(options)));

        let download_task_clone = download_task.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            download_task_clone.lock().await.cancel();
        });

        let result = {
            let mut task = download_task.lock().await;
            task.run().await
        };

        let end_cause = result?.await?;
        match end_cause {
            DownloadEndCause::Finished => {

            }
            DownloadEndCause::Canceled => {
                let task_guard = download_task.lock().await;
                let archive_data = task_guard.archive().await.unwrap();
                let mut file = tokio::fs::File::create("C:/Users/User/Downloads/log.json").await?;
                let vev_json = serde_json::to_vec_pretty(&archive_data)?;
                file.write_all(&vev_json).await?;
            }
        };

        Ok(())
    }
}
