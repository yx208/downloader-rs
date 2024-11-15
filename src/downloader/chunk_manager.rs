use std::collections::HashMap;
use std::future::Future;
use std::num::NonZeroU8;
use std::sync::Arc;

use futures_util::stream::FuturesUnordered;
use futures_util::StreamExt;
use reqwest::{Client, Request};
use tokio::fs::File;
use tokio::sync::{Mutex, RwLock};
use anyhow::Result;
use tokio_util::sync::CancellationToken;
use crate::downloader::chunk_item::ChunkItem;
use crate::downloader::chunk_iterator::ChunkIterator;
use crate::downloader::error::{DownloadEndCause, DownloadError};

enum DownloadFutureResult {
    DownloadEnd,
    DownloadCanceled,
    DownloadPaused
}

pub struct ChunkManger {
    connection_count: u8,
    retry_count: u8,
    chunk_iter: ChunkIterator,
    client: Client,
    cancel_token: CancellationToken,
    downloading_chunks: RwLock<HashMap<usize, Arc<ChunkItem>>>
}

impl ChunkManger {
    pub fn new(
        connection_count: NonZeroU8,
        retry_count: u8,
        chunk_iter: ChunkIterator,
        client: Client,
        cancel_token: CancellationToken
    ) -> Self {
        Self {
            retry_count,
            chunk_iter,
            client,
            cancel_token,
            connection_count: connection_count.get(),
            downloading_chunks: RwLock::new(HashMap::new())
        }
    }

    pub fn clone_request(request: &Request) -> Request {
        let mut req = Request::new(request.method().clone(), request.url().clone());
        *req.headers_mut() = request.headers().clone();
        *req.version_mut() = request.version();
        *req.timeout_mut() = request.timeout().map(Clone::clone);

        req
    }

    pub async fn start_download(&mut self, file: File, request: Request) -> Result<DownloadEndCause, DownloadError> {
        let mut future_unordered = FuturesUnordered::new();
        let file = Arc::new(Mutex::new(file));

        for _ in 0..self.connection_count {
            match self.download_next_chunk(file.clone(), ChunkManger::clone_request(&request)).await {
                Some((_, future)) => {
                    future_unordered.push(future);
                }
                None => break
            }
        }

        let mut result = Ok(DownloadEndCause::Finished);
        let mut is_iter_finish = false;
        while let Some(future_result) = future_unordered.next().await {
            // 等待 chunk 完成
            match future_result {
                Ok(DownloadEndCause::Finished) => {
                    if is_iter_finish {
                        if self.downloading_chunk_count().await == 0 {
                            break;
                        }
                    } else {

                    }

                    is_iter_finish = true;
                }
                Ok(DownloadEndCause::Canceled) => {
                    result = Ok(DownloadEndCause::Canceled);
                }
                Err(err) => {
                    result = Err(err);
                    self.cancel_token.cancel();
                }
            }
        }

        result
    }

    async fn downloading_chunk_count(&self) -> usize {
        self.downloading_chunks.read().await.len()
    }

    async fn insert_chunk(&mut self, chunk: Arc<ChunkItem>) {
        let mut downloading_chunks = self.downloading_chunks.write().await;
        downloading_chunks.insert(chunk.chunk_info.index, chunk);
    }

    async fn download_next_chunk(&mut self, file: Arc<Mutex<File>>, request: Request)
        -> Option<(usize, impl Future<Output=Result<DownloadEndCause, DownloadError>>)>
    {
        if let Some(chunk_info) = self.chunk_iter.next() {
            let chunk_item = Arc::new(
                ChunkItem::new(
                    self.client.clone(),
                    chunk_info,
                    file,
                    self.cancel_token.child_token()
                )
            );

            self.insert_chunk(chunk_item.clone()).await;
            Some((chunk_item.chunk_info.index, chunk_item.download(request, self.retry_count)))
        } else {
            None
        }
    }
}

mod tests {
    use super::*;

    #[tokio::test]
    async fn should_be_run() {

    }
}
