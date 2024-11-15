use tokio::fs::File;
use std::sync::Arc;
use anyhow::Result;
use bytes::Bytes;
use reqwest::Response;
use tokio::io::AsyncWriteExt;
use tokio::select;
use tokio_util::sync::CancellationToken;
use crate::downloader::chunk_manager::ChunkManager;
use crate::downloader::downloader::{DownloadError, DownloadingEndCause};

pub struct SingleDownload {
    cancel_token: CancellationToken,
    pub content_length: Option<u64>,
}

impl SingleDownload {
    pub fn new(
        cancel_token: CancellationToken,
        content_length: Option<u64>
    ) -> Self {
        Self {
            cancel_token,
            content_length
        }
    }

    pub async fn download(
        &self,
        mut file: File,
        response: Box<Response>,
        // downloaded_len_receiver: Option<Arc<dyn DownloadedLenChangeNotify>>,
        buffer_size: usize
    ) -> Result<DownloadingEndCause, DownloadError> {
        use futures_util::StreamExt;

        let mut chunk_bytes = Vec::with_capacity(buffer_size);
        let future = async {
            let mut stream = response.bytes_stream();
            while let Some(bytes) = stream.next().await {
                let bytes: Bytes = {
                    match bytes {
                        Ok(bytes) => bytes,
                        Err(err) => {
                            return Err(DownloadError::HttpRequestFailed(err));
                        }
                    }
                };
                let len = bytes.len();

                if chunk_bytes.len() + len > chunk_bytes.capacity() {
                    file.write_all(&chunk_bytes).await?;
                    file.flush().await?;
                    file.sync_all().await?;
                    chunk_bytes.clear();
                }

                chunk_bytes.extend(bytes);
                // 发送长度变化通知
                // self.downloaded_len_sender.send_modify(|n| *n += len as u64);
                // if let Some(downloaded_len_receiver) = downloaded_len_receiver.as_ref() {
                //     downloaded_len_receiver.receive_len(len).await;
                // }
            }

            Result::<(), DownloadError>::Ok(())
        };

        Ok(select! {
            r = future => {
                r?;

                file.write_all(&chunk_bytes).await?;
                file.flush().await?;
                file.sync_all().await?;

                DownloadingEndCause::DownloadFinished
            }
            _ = self.cancel_token.cancelled() => {
                DownloadingEndCause::Cancelled
            }
        })
    }
}

pub enum DownloadWay {
    Ranges(Arc<ChunkManager>),
    Single(SingleDownload),
}