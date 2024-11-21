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
use crate::download::error::{DownloadActionNotify, DownloadEndCause, DownloadError};
use crate::download::util::clone_request;

type DownloadResultType = Result<DownloadEndCause, DownloadError>;

pub struct ChunkManager {
    client: Client,
    connection_count: u8,
    pub chunk_iter: ChunkRangeIterator,
    pub downloading_chunks: RwLock<HashMap<usize, Arc<ChunkItem>>>,
    action_receiver: watch::Receiver<DownloadActionNotify>,
    action_sender: watch::Sender<DownloadActionNotify>,
}

impl ChunkManager {
    pub fn new(
        connection_count: u8,
        chunk_iter: ChunkRangeIterator,
        action_sender: watch::Sender<DownloadActionNotify>,
        action_receiver: watch::Receiver<DownloadActionNotify>,
    ) -> Self {
        Self {
            chunk_iter,
            connection_count,
            action_receiver,
            action_sender,
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

    pub async fn get_downloading_chunks(&self) -> Vec<Arc<ChunkItem>> {
        let chunks = self.downloading_chunks.read().await;
        let mut chunks: Vec<_> = chunks.values().cloned().collect();
        chunks.sort_by(|a, b| a.chunk_info.range.start.cmp(&b.chunk_info.range.start));

        chunks
    }

    pub async fn download(
        &self,
        file: File,
        request: Request,
        retry_count: u8,
        downloaded_len_sender: watch::Sender<u64>,
    ) -> DownloadResultType {
        let mut futures_unordered = FuturesUnordered::new();
        let mut is_download_finished = false;
        let file = Arc::new(Mutex::new(file));

        let download_next_chunk  = || async {
            self.download_next_chunk(
                file.clone(),
                retry_count,
                clone_request(&request),
                downloaded_len_sender.clone()
            ).await
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
                // 发生错误时通通知其他 chunk 停止下载
                Err(err) => {
                    match self.action_sender.send(DownloadActionNotify::Error) {
                        Ok(_) => {}
                        Err(_) => {}
                    };
                    return Err(err);
                }
            }
        }

        Ok(DownloadEndCause::Finished)
    }

    async fn download_next_chunk(
        &self,
        file: Arc<Mutex<File>>,
        retry_count: u8,
        request: Request,
        downloaded_len_sender: watch::Sender<u64>
    ) -> Option<BoxFuture<(usize, DownloadResultType)>> {
        if let Some(chunk_info) = self.chunk_iter.next() {
            let chunk_item = Arc::new(ChunkItem::new(
                chunk_info,
                file,
                self.client.clone(),
                retry_count
            ));
            self.insert_downloading_chunk(chunk_item.clone()).await;

            let future = async move {
                let result = chunk_item.download(
                    request,
                    self.action_receiver.clone(),
                    downloaded_len_sender
                ).await;
                (chunk_item.chunk_info.index, result)
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
        let (tx, rx) = watch::channel(
            DownloadActionNotify::Notify(DownloadEndCause::Finished)
        );
        let chunk_manager = ChunkManager::new(3, chunk_iter, tx, rx);

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
        let file = create_file().await;
        let (len_tx, _len_rx) = watch::channel::<u64>(0);
        let result = chunk_manager.download(
            file,
            Request::new(reqwest::Method::GET, url.clone()),
            3,
            len_tx
        );

        match result.await {
            Ok(download_end_cause) => {
                println!("Success: {:?}", download_end_cause);
            }
            Err(err) => {
                println!("Error: {:?}", err);
            }
        }
    }
}
