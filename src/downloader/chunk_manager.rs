use std::collections::HashMap;
use std::future::Future;
use std::num::NonZeroU8;
use std::sync::Arc;

use futures_util::stream::FuturesUnordered;
use futures_util::{FutureExt, StreamExt};
use reqwest::{Client, Request};
use tokio::fs::File;
use tokio::sync::{Mutex, RwLock};
use anyhow::Result;
use futures_util::future::BoxFuture;
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

    pub async fn start_download(&self, file: File, request: Request) -> Result<DownloadEndCause, DownloadError> {
        let mut future_unordered = FuturesUnordered::new();
        let file = Arc::new(Mutex::new(file));

        let mut result = Ok(DownloadEndCause::Finished);
        let mut is_iter_finish = false;

        for _ in 0..self.connection_count {
            match self.download_next_chunk(file.clone(), ChunkManger::clone_request(&request)).await {
                Some(future) => future_unordered.push(future),
                None => {
                    // 默认的连接数可以使 chunk 切完，说明没有了
                    is_iter_finish = true;
                    break;
                }
            }
        }

        while let Some((chunk_index, future)) = future_unordered.next().await {
            // 等待 chunk 完成
            match future {
                Ok(DownloadEndCause::Finished) => {
                    let (downloading_count, ..) = self.remove_chunk(chunk_index).await;

                    if is_iter_finish {
                        if self.downloading_chunk_count().await == 0 {
                            break;
                        }
                    } else {
                        match self.download_next_chunk(file.clone(), ChunkManger::clone_request(&request)).await {
                            Some(future) => future_unordered.push(future),
                            None => {
                                is_iter_finish = true;
                                if downloading_count == 0 {
                                    break;
                                }
                            }
                        }
                    }
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

    async fn insert_chunk(&self, chunk: Arc<ChunkItem>) {
        let mut downloading_chunks = self.downloading_chunks.write().await;
        downloading_chunks.insert(chunk.chunk_info.index, chunk);
    }

    async fn remove_chunk(&self, index: usize) -> (usize, Option<Arc<ChunkItem>>) {
        let mut downloading_chunks = self.downloading_chunks.write().await;
        let chunk = downloading_chunks.remove(&index);

        (downloading_chunks.len(), chunk)
    }

    async fn download_next_chunk(&self, file: Arc<Mutex<File>>, request: Request)
        -> Option<BoxFuture<(usize, Result<DownloadEndCause, DownloadError>)>>
    {
        if let Some(chunk_info) = self.chunk_iter.next() {
            let chunk_index = chunk_info.index;
            let chunk_item = Arc::new(ChunkItem::new(
                self.client.clone(),
                chunk_info,
                file,
                self.cancel_token.child_token()
            ));
            self.insert_chunk(chunk_item.clone()).await;

            let future = Box::pin(async move {
                let result = chunk_item.download(request, self.retry_count).await;
                (chunk_index, result)
            });

            Some(future)
        } else {
            None
        }
    }
}

mod tests {
    use super::*;

    use std::num::NonZeroUsize;
    use tokio::fs::OpenOptions;
    use url::Url;
    use crate::downloader::chunk_iterator::{ChunkIteratorData, RemainingChunks};
    use crate::downloader::util::get_file_length;

    #[tokio::test]
    async fn should_be_run() -> Result<()> {
        let file_url = Url::parse("http://localhost:23333/image.jpg")?;
        let file_size = get_file_length(file_url.clone()).await.unwrap();
        let mut remaining = RemainingChunks::new(NonZeroUsize::new(1024 * 1024 * 4).unwrap(), file_size);

        let chunk_iter = ChunkIterator::new(ChunkIteratorData { iter_count: 0, remaining });
        let mut chunk_manger = ChunkManger::new(
            NonZeroU8::new(10).unwrap(),
            3,
            chunk_iter,
            Client::new(),
            CancellationToken::new()
        );

        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .open("C:/Users/X/Downloads/demo.jpg")
            .await?;
        let request = Request::new(reqwest::Method::GET, file_url);
        chunk_manger.start_download(file, request).await?;

        Ok(())
    }
}
