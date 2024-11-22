use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use uuid::Uuid;
use crate::download::archiver::Archiver;
use crate::download::chunk_range::{ChunkInfo, ChunkRange};
use crate::download::downloader::{Downloader, DownloaderConfig};
use crate::download::error::{DownloadEndCause, DownloadError, DownloadStartError};

type DownloadResultType = Result<DownloadEndCause, DownloadError>;

pub enum DownloadStatus {
    Pending,
    Running,
    Paused,
    Cancelled
}

pub struct DownloadTask {
    id: Uuid,
    status:DownloadStatus,
    downloader: Option<Downloader>,
    archiver: Option<Archiver>,
    config: Arc<DownloaderConfig>
}

impl DownloadTask {
    pub fn new(id: Uuid, config: DownloaderConfig, ) -> Self {
        Self {
            id,
            status: DownloadStatus::Pending,
            config: Arc::new(config),
            downloader: None,
            archiver: None,
        }
    }

    pub async fn exec(&mut self, action: DownloadEndCause) {
        if let Some(mut downloader) = self.downloader.take() {
            let chunk_manager = downloader.exec(action).unwrap();
            let downloaded_len = {
                let guard = chunk_manager.chunk_iter.data.read();
                guard.remaining_len()
            };
            let mut chunk_data = {
                let guard = chunk_manager.chunk_iter.data.read();
                guard.clone()
            };
            let downloading_chunks = chunk_manager.get_downloading_chunks().await;
            // 把当前下载中的 chunk，添加进队列
            chunk_data.last_incomplete_chunks.extend(
                downloading_chunks.iter().filter_map(|chunk| {
                    let downloaded = chunk.downloaded.load(Ordering::SeqCst);
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
            );

            self.archiver = Some(Archiver {
                downloaded_len,
                chunk_data: Some(chunk_data),
            });
        }
    }

    pub async fn download(&mut self) -> Result<impl Future<Output=DownloadResultType>, DownloadStartError> {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Uuid>(32);
        tx.send(self.id).await.unwrap();

        let mut downloader = Downloader::new();
        let archiver = {
            if self.archiver.is_some() {
                Some(self.archiver.as_ref().unwrap())
            } else {
                None
            }
        };
        let future = downloader.download(self.config.clone(), archiver).await;
        self.downloader = Some(downloader);

        future
    }

    pub async fn pause(&mut self) {
        self.exec(DownloadEndCause::Paused).await;
    }

    pub async fn cancel(&mut self) {
        self.exec(DownloadEndCause::Cancelled).await;
    }
}

mod tests {
    #[tokio::test]
    async fn should_be_run() {

    }
}
