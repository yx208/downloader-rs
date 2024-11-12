use std::io::SeekFrom;
use std::sync::{Arc};
use std::sync::atomic::{AtomicU64, Ordering};
use headers::HeaderMapExt;
use anyhow::Result;
use futures_util::StreamExt;
use bytes::Bytes;
use tokio_util::sync::CancellationToken;
use tokio::{
    sync::{Mutex},
    fs::File,
    io::{AsyncSeekExt, AsyncWriteExt},
    select,
};

use crate::downloader::chunk_iterator::{ChunkInfo, ChunkRange};
use crate::downloader::chunk_manager::ChunkManager;
use crate::downloader::downloader::{DownloadError, DownloadingEndCause};

pub struct ChunkItem {
    pub chunk_info: ChunkInfo,
    pub downloaded_len: AtomicU64,
    client: reqwest::Client,
    file: Arc<Mutex<File>>,
    cancel_token: CancellationToken
}

impl ChunkItem {
    pub fn new(chunk_info: ChunkInfo, cancel_token: CancellationToken, file: Arc<Mutex<File>>, client: reqwest::Client) -> Self {
        Self {
            file,
            client,
            chunk_info,
            cancel_token,
            downloaded_len: AtomicU64::new(0)
        }
    }

    pub fn add_downloaded_len(&self, len: usize) {
        self.downloaded_len.fetch_add(len as u64, Ordering::Relaxed);

        debug_assert!(
            self.downloaded_len.load(Ordering::SeqCst) <= self.chunk_info.range.len(),
            "downloaded_len: {},chunk_info.range.len(): {}",
            self.downloaded_len.load(Ordering::SeqCst),
            self.chunk_info.range.len()
        );
    }

    pub(crate) async fn download_chunk(
        self: Arc<Self>,
        mut request: Box<reqwest::Request>,
        retry_count: u8
    ) -> Result<DownloadingEndCause, DownloadError> {
        let cancel_token = self.cancel_token.clone();
        let mut chunk_bytes = Vec::with_capacity(self.chunk_info.range.len() as usize);

        let mut current_retry_count: u8 = 0;
        let future = async {
            'r: loop {
                // 更改请求的数据范围
                request.headers_mut().typed_insert(
                    ChunkRange::new(
                        self.chunk_info.range.start + chunk_bytes.len() as u64, self.chunk_info.range.end
                    ).to_range_header()
                );

                let response = self.client.execute(*ChunkManager::clone_request(&request));
                let response = match response.await {
                    Ok(response) => {
                        current_retry_count = 0;
                        response
                    },
                    Err(err) => {
                        current_retry_count += 1;
                        if current_retry_count > retry_count {
                            return Err(DownloadError::HttpRequestFailed(err));
                        }

                        continue 'r;
                    }
                };

                let mut stream = response.bytes_stream();
                while let Some(bytes) = stream.next().await {
                    let bytes: Bytes = match bytes {
                        Ok(unwrap_bytes) => {
                            current_retry_count = 0;
                            unwrap_bytes
                        }
                        Err(err) => {
                            current_retry_count += 1;

                            // 无论是出错还是取消，都需要写入磁盘进行持久化
                            if current_retry_count > retry_count {
                                let mut file = self.file.lock().await;
                                file.seek(SeekFrom::Start(self.chunk_info.range.start)).await?;
                                file.write_all(chunk_bytes.as_ref()).await?;
                                file.flush().await?;
                                file.sync_all().await?;

                                return Err(DownloadError::HttpRequestFailed(err))
                            }

                            continue 'r;
                        }
                    };

                    let len = bytes.len();
                    chunk_bytes.extend(bytes);
                    self.add_downloaded_len(len);
                }

                break;
            }

            Result::<(), DownloadError>::Ok(())
        };

        select! {
            result = future => {
                result?;

                let mut file = self.file.lock().await;
                file.seek(SeekFrom::Start(self.chunk_info.range.start)).await?;
                file.write_all(chunk_bytes.as_ref()).await?;
                file.flush().await?;
                file.sync_all().await?;

                Ok(DownloadingEndCause::DownloadFinished)
            }
            _ = cancel_token.cancelled() => {
                let mut file = self.file.lock().await;
                file.seek(SeekFrom::Start(self.chunk_info.range.start)).await?;
                file.write_all(chunk_bytes.as_ref()).await?;
                file.flush().await?;
                file.sync_all().await?;

                Ok(DownloadingEndCause::Cancelled)
            }
        }
    }
}
