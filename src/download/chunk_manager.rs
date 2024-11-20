use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use futures_util::future::BoxFuture;
use futures_util::stream::FuturesUnordered;
use futures_util::{FutureExt, StreamExt};
use reqwest::{Client, Request};
use tokio::fs::File;
use tokio::sync::{watch, Mutex, RwLock};
use crate::download::chunk_item::ChunkItem;
use crate::download::chunk_range::ChunkRangeIterator;
use crate::download::error::{DownloadEndCause, DownloadError};
use crate::download::util::clone_request;

type DownloadResultType = Result<DownloadEndCause, DownloadError>;

pub struct ChunkManager {
    chunk_iter: ChunkRangeIterator,
    client: Client,
    connection_count: u8,
    downloading_chunks: RwLock<HashMap<usize, Arc<ChunkItem>>>
}

impl ChunkManager {
    pub fn new(connection_count: u8, chunk_iter: ChunkRangeIterator) -> Self {
        Self {
            chunk_iter,
            connection_count,
            client: Client::new(),
            downloading_chunks: RwLock::new(HashMap::new())
        }
    }

    async fn insert_downloading_chunk(&self, chunk_item: Arc<ChunkItem>) {
        let mut guard = self.downloading_chunks.write().await;
        guard.insert(chunk_item.chunk_info.index, chunk_item);
    }

    /// 从下载列表中移除 chunk
    /// 返回正在下载的 chunk 数量
    async fn remove_downloading_chunk(&self, chunk_index: usize) -> usize {
        let mut guard = self.downloading_chunks.write().await;
        guard.remove(&chunk_index);

        guard.len()
    }

    pub async fn download(
        &self,
        file: Arc<Mutex<File>>,
        request: Request,
        action_receiver: watch::Receiver<DownloadEndCause>
    ) -> DownloadResultType {
        let mut futures_unordered = FuturesUnordered::new();
        let mut is_download_finished = false;

        let download_next_chunk  = || async {
            self.download_next_chunk(file.clone(), clone_request(&request), action_receiver.clone()).await
        };

        // 下载连接数的 chunk
        for _ in 0..self.connection_count {
            match download_next_chunk().await {
                None => {
                    is_download_finished = true;
                    break;
                }
                Some(future) => futures_unordered.push(future)
            }
        }

        while let Some((chunk_index, result)) = futures_unordered.next().await {
            // 当有 chunk 下载完成
            let downloading_count = self.remove_downloading_chunk(chunk_index).await;
            match result {
                Ok(DownloadEndCause::Finished) => {
                    // 没有 NextChunk 并且也没有下载中的 chunk 即可退出 while 循环
                    if is_download_finished && downloading_count == 0 {
                        break;
                    }

                    match download_next_chunk().await {
                        None => {
                            is_download_finished = true;
                            if downloading_count == 0 {
                                break;
                            }
                        },
                        Some(future) => {
                            futures_unordered.push(future);
                        }
                    }
                }
                Ok(DownloadEndCause::Cancelled) => {
                    return Ok(DownloadEndCause::Cancelled);
                }
                Ok(DownloadEndCause::Paused) => {
                    return Ok(DownloadEndCause::Paused);
                }
                Err(err) => return Err(err)
            }
        }

        Ok(DownloadEndCause::Finished)
    }

    async fn download_next_chunk(
        &self,
        file: Arc<Mutex<File>>,
        request: Request,
        action_receiver: watch::Receiver<DownloadEndCause>
    ) -> Option<BoxFuture<(usize, DownloadResultType)>> {
        if let Some(chunk_info) = self.chunk_iter.next() {
            println!("Chunk info: {:?}", chunk_info);
            let chunk_item = Arc::new(ChunkItem::new(
                chunk_info,
                file,
                self.client.clone(),
                3
            ));
            self.insert_downloading_chunk(chunk_item.clone()).await;

            let future = async move {
                let res = chunk_item.download(request, action_receiver).await;
                (chunk_item.chunk_info.index, res)
            };

            Some(future.boxed())
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
    use crate::download::chunk_range::ChunkData;
    use crate::download::util::get_file_length;

    async fn create_manager(url: Url) -> ChunkManager {
        let content_length = get_file_length(url).await.unwrap();
        let chunk_size = NonZeroUsize::new(1024 * 1024 * 4).unwrap();
        let chunk_data = ChunkData::new(chunk_size, content_length);
        let chunk_iter = ChunkRangeIterator::new(chunk_data);
        let chunk_manager = ChunkManager::new(3, chunk_iter);

        chunk_manager
    }

    async fn create_file() -> File {
        let mut download_dir = dirs::download_dir().unwrap();
        download_dir.push("demo.jpg");
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(download_dir)
            .await
            .unwrap();

        file
    }

    #[tokio::test]
    async fn should_be_download() {
        let url = Url::parse("http://localhost:23333/image.jpg").unwrap();
        let chunk_manager = create_manager(url.clone()).await;
        let file = Arc::new(Mutex::new(create_file().await));
        let (tx, rx) = watch::channel(DownloadEndCause::Finished);

        let result = chunk_manager.download(
            file.clone(),
            Request::new(reqwest::Method::GET, url.clone()),
            rx.clone(),
        );

        match result.await {
            Ok(download_end_cause) => {
                println!("{:?}", download_end_cause);
            }
            Err(err) => {
                println!("{}", err);
            }
        }
    }
}
