use std::io::SeekFrom;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use futures_util::StreamExt;
use headers::HeaderMapExt;
use reqwest::{Client, Request};
use tokio::fs::File;
use tokio::sync::Mutex;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::downloader::chunk_info::ChunkInfo;
use crate::downloader::chunk_manager::ChunkManger;
use crate::downloader::chunk_range::ChunkRange;
use crate::downloader::error::{DownloadEndCause, DownloadError};

pub struct ChunkItem {
    pub chunk_info: ChunkInfo,
    pub downloaded: AtomicU64,
    client: Client,
    file: Arc<Mutex<File>>,
    cancel_token: CancellationToken
}

impl ChunkItem {
    pub fn new(
        client: Client,
        chunk_info: ChunkInfo,
        file: Arc<Mutex<File>>,
        cancel_token: CancellationToken,
    ) -> Self {
        Self {
            file,
            client,
            chunk_info,
            cancel_token,
            downloaded: AtomicU64::new(0),
        }
    }

    pub async fn download(&self, request: Request, max_retry_count: u8) -> Result<DownloadEndCause, DownloadError> {
        let future = self.execute_download(request, max_retry_count);
        tokio::select! {
            res = future => {
                match res {
                    Ok(bytes) => {
                        self.write_bytes_to_file(bytes.as_ref()).await?;
                        Ok(DownloadEndCause::Finished)
                    }
                    Err(err) => {
                        Err(err)
                    }
                }
            }
            _ = self.cancel_token.cancelled() => {
                println!("Cancelled chunk item: {}", self.chunk_info.index);
                Ok(DownloadEndCause::Canceled)
            }
        }
    }

    async fn execute_download(&self, mut request: Request, max_retry_count: u8) -> Result<Vec<u8>, DownloadError> {
        let mut retry_count = 0;
        let mut chunk_bytes = Vec::with_capacity(self.chunk_info.range.len() as usize);

        'r: loop {
            request.headers_mut().typed_insert(
                ChunkRange::new(
                    self.chunk_info.range.start + chunk_bytes.len() as u64,
                    self.chunk_info.range.end
                ).to_range_header()
            );
            let response = match self.client.execute(ChunkManger::clone_request(&request)).await {
                Ok(response) => {
                    retry_count = 0;
                    response
                }
                Err(err) => {
                    retry_count += 1;

                    if retry_count > max_retry_count {
                        return Err(DownloadError::HttpRequestError(err));
                    }

                    continue 'r;
                }
            };

            let mut stream = response.bytes_stream();
            while let Some(bytes) = stream.next().await {
                let bytes = match bytes {
                    Ok(bytes) => {
                        bytes
                    }
                    Err(err) => {
                        retry_count += 1;

                        if retry_count > max_retry_count {
                            self.write_bytes_to_file(chunk_bytes.as_ref()).await?;
                            return Err(DownloadError::HttpRequestError(err));
                        }

                        continue 'r;
                    }
                };

                let len = bytes.len();
                chunk_bytes.extend(bytes);
                self.add_downloaded_len(len as u64);
                // TODO: 通知上层，当前下载了多大
            }

            break;
        }

        Ok(chunk_bytes)
    }

    fn add_downloaded_len(&self, len: u64) {
        self.downloaded.fetch_add(len, Ordering::Relaxed);
    }

    async fn write_bytes_to_file<'a>(&'a self, bytes: &'a [u8]) -> Result<(), DownloadError> {
        let mut file = self.file.lock().await;
        file.seek(SeekFrom::Start(self.chunk_info.range.start)).await?;
        file.write_all(bytes.as_ref()).await?;
        file.flush().await?;
        file.sync_all().await?;

        Ok(())
    }
}

mod tests {
    use tokio::fs::OpenOptions;
    use super::*;
    use url::Url;
    use crate::downloader::chunk_range::ChunkRange;

    #[tokio::test]
    async fn should_be_run() {
        let token = CancellationToken::new();
        let client = Client::new();
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .open("C:/Users/User/Downloads/test.jpg")
            .await
            .unwrap();

        let chunk_range = ChunkRange::new(0, 1024 * 1024 *4);
        let chunk_info = ChunkInfo { index: 0, range: chunk_range };
        let item = ChunkItem::new(client, chunk_info, Arc::new(Mutex::new(file)), token.clone());

        let file_url = "http://localhost:23333/image.jpg";
        let mut request = Request::new(reqwest::Method::GET, Url::parse(file_url).unwrap());
        let header_map = request.headers_mut();
        header_map.typed_insert(chunk_range.to_range_header());

        item.download(request, 3).await.unwrap();
    }
}
