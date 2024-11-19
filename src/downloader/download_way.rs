use reqwest::{Client, Request};
use anyhow::Result;
use futures_util::StreamExt;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::select;
use tokio_util::sync::CancellationToken;
use crate::downloader::chunk_manager::ChunkManger;
use crate::downloader::error::{DownloadEndCause, DownloadError};

pub struct DownloadSingle {
    cancel_token: CancellationToken,
    client: Client,
    pub file_size: u64
}

impl DownloadSingle {
    pub fn new(cancel_token: CancellationToken, file_size: u64) -> Self {
        Self {
            cancel_token,
            file_size,
            client: Client::new()
        }
    }

    pub async fn download(&self, mut file: File, request: Request, buffer_size: usize)
        -> Result<DownloadEndCause, DownloadError>
    {
        let mut chunk_bytes = Vec::with_capacity(buffer_size);
        let future = async {
            let response = match self.client.execute(ChunkManger::clone_request(&request)).await {
                Ok(response) => response,
                Err(err) => {
                    return Err(DownloadError::HttpRequestError(err));
                }
            };

            let mut stream = response.bytes_stream();
            while let Some(bytes) = stream.next().await {
                let bytes = match bytes {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        return Err(DownloadError::HttpRequestError(err));
                    }
                };
                let bytes_len = bytes.len();

                if chunk_bytes.len() + bytes_len > chunk_bytes.capacity() {
                    file.write_all(&chunk_bytes).await?;
                    file.flush().await?;
                    file.sync_all().await?;
                    chunk_bytes.clear();
                }
            }

            Ok(())
        };

        Ok(select! {
            r = future => {
                r?;
                file.write_all(&chunk_bytes).await?;
                file.flush().await?;
                file.sync_all().await?;
                DownloadEndCause::Finished
            },
            _ = self.cancel_token.cancelled() => {
                DownloadEndCause::Canceled
            }
        })
    }
}

pub enum DownloadWay {
    Range(ChunkManger),
    Single(DownloadSingle)
}