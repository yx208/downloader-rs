use std::future::Future;
use std::num::{NonZeroU8, NonZeroUsize};
use std::path::PathBuf;

use tokio_util::sync::CancellationToken;
use url::Url;
use anyhow::Result;

use crate::downloader::downloader::{DownloaderConfig, FileDownloader};
use crate::downloader::error::{DownloadEndCause, DownloadError, DownloadStartError};

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

    pub fn cancel(&self) {
        self.downloader.cancel();
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
    use url::Url;
    use crate::task::task::{DownloadTask, TaskOptions};

    #[tokio::test]
    async fn should_be_cancel() {
        let options = TaskOptions {
            url: Url::parse("http://localhost:23333/image.jpg").unwrap(),
            file_size: 14_674_216,
            save_dir: PathBuf::from("C:/Users/X/Downloads"),
            filename: "demo.jpg".to_string(),
        };
        let mut download_task = DownloadTask::new(options);
        match download_task.run().await {
            Ok(future) => {
                match future.await {
                    Ok(result) => {
                        println!("{:?}", result);
                    }
                    Err(err) => {
                        println!("{}", err);
                    }
                }
            }
            Err(err) => {
                println!("Start error: {}", err);
            }
        }
    }
}
