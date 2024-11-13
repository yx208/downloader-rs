use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::task::{Context, Poll};

// 将 Future 装箱（Boxed）为 `dyn Future`
use futures_util::future::BoxFuture;
use futures_util::{FutureExt, StreamExt};
use futures_util::stream::FuturesUnordered;
use reqwest::{Request, Client};
use tokio_util::sync::CancellationToken;
use tokio::sync::Mutex;
use tokio::sync::watch;
use tokio::fs::File;

use crate::downloader::chunk_item::ChunkItem;
use crate::downloader::chunk_iterator::{ChunkIterator, ChunkRange};
use crate::downloader::downloader::{DownloadError, DownloadingEndCause};

///提供给外部查看信息用的结构体
pub struct ChunksInfo {
    finished_chunks: Vec<ChunkRange>,
    downloading_chunks: Vec<ChunkItem>,
    // 没有剩余的块
    no_chunk_remaining: bool,
}

type ChunkMapIndex = usize;
pub struct ChunkManager {
    pub retry_count: u8,
    pub chunk_iterator: ChunkIterator,

    /// 下载连接数，生产消费模型
    /// 生产者
    download_connection_count_sender: watch::Sender<u8>,
    /// 消费者
    pub download_connection_count_receiver: watch::Receiver<u8>,

    downloading_chunks: Mutex<HashMap<ChunkMapIndex, Arc<ChunkItem>>>,
    /// 剩余连接数
    pub superfluities_connection_count: AtomicU8,
    client: Client,
    cancel_token: CancellationToken
}

impl ChunkManager {
    pub fn new(
        connection_count: u8,
        client: Client,
        cancel_token: CancellationToken,
        chunk_iterator: ChunkIterator,
        retry_count: u8
    ) -> Self {
        let (tx, rx) = watch::channel(connection_count);

        Self {
            client,
            cancel_token,
            chunk_iterator,
            retry_count,
            downloading_chunks: Mutex::new(HashMap::new()),
            download_connection_count_receiver: rx,
            download_connection_count_sender: tx,
            superfluities_connection_count: AtomicU8::new(0),
        }
    }

    pub fn clone_request(request: &Request) -> Box<Request> {
        let mut req = Request::new(request.method().clone(), request.url().clone());
        *req.headers_mut() = request.headers().clone();
        *req.version_mut() = request.version();
        *req.timeout_mut() = request.timeout().map(Clone::clone);
        Box::new(req)
    }

    pub async fn start_download(&self, file: File, request: Box<Request>) -> Result<DownloadingEndCause, DownloadError> {
        #[derive(Debug)]
        enum RunFutureResult {
            DownloadConnectionCountChanged {
                receiver: watch::Receiver<u8>,
                download_connection_count: u8
            },
            ChunkDownloadEnd {
                chunk_index: usize,
                result: Result<DownloadingEndCause, DownloadError>,
            }
        }

        enum RunFuture<'a> {
            DownloadConnectionCountChanged(BoxFuture<'a, (watch::Receiver<u8>, u8)>),
            ChunkDownloadEnd {
                chunk_index: usize,
                future: BoxFuture<'a, Result<DownloadingEndCause, DownloadError>>
            }
        }

        // 轮询结果转为对应的 ResultFuture
        impl Future for RunFuture<'_> {
            type Output = RunFutureResult;

            // 检查 future 是否完成
            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                match self.get_mut() {
                    RunFuture::DownloadConnectionCountChanged(future) => {
                        // 对 BoxFuture 进行轮询
                        // 如果 Future 为 Ready 状态则转为 RunFutureResult::DownloadConnectionCountChanged
                        future.poll_unpin(cx).map(|(rx, count)| {
                            RunFutureResult::DownloadConnectionCountChanged {
                                receiver: rx,
                                download_connection_count: count
                            }
                        })
                    }
                    RunFuture::ChunkDownloadEnd { chunk_index, future } => {
                        // 对 BoxFuture 进行轮询
                        // 如果 Future 为 Ready 状态则转为 RunFutureResult::ChunkDownloadEnd
                        future.poll_unpin(cx).map(|result| {
                            RunFutureResult::ChunkDownloadEnd {
                                chunk_index: chunk_index.clone(),
                                result
                            }
                        })
                    }
                }
            }
        }

        // 用于高效地并发执行多个 Future，并按完成顺序返回结果
        // 这里的 FuturesUnordered 可以接受 RunFuture 类型，也就是这个枚举的两种 Future
        let mut futures_unordered = FuturesUnordered::new();

        let file = Arc::new(Mutex::new(file));
        let download_next_chunk = || async {
            match self.download_next_chunk(file.clone(), ChunkManager::clone_request(&request)).await {
                Some((chunk_index, future)) => {
                    Some(RunFuture::ChunkDownloadEnd {
                        chunk_index,
                        future: future.boxed()
                    })
                }
                // 如果返回 None，则说明没有 chunk 了
                None => {
                    None
                }
            }
        };
        match download_next_chunk().await {
            // 添加下载任务到队列
            Some(future) => futures_unordered.push(future),
            None => {
                // 没有 chunk 则说明完成了下载
                return Ok(DownloadingEndCause::DownloadFinished)
            }
        }

        let mut is_iter_finished = false;
        // 迭代所有连接数
        for _ in 0..(self.connection_count() - 1) {
            match download_next_chunk().await {
                Some(future) => futures_unordered.push(future),
                // 没有其他 chunk
                None => {
                    is_iter_finished = true;
                    break;
                }
            }
        }

        futures_unordered.push(RunFuture::DownloadConnectionCountChanged({
            let mut receiver = self.download_connection_count_receiver.clone();
            async move {
                let _ = receiver.changed().await;
                let i = *receiver.borrow();
                (receiver, i)
            }.boxed()
        }));

        let mut result = Result::<DownloadingEndCause, DownloadError>::Ok(DownloadingEndCause::DownloadFinished);
        // 匹配不同 futureResult 产生的结果，进行处理
        while let Some(future_result) = futures_unordered.next().await {
            match future_result {
                // 连接数变更
                RunFutureResult::DownloadConnectionCountChanged {
                    download_connection_count,
                    mut receiver
                } => {
                    // 变更到 0
                    if download_connection_count == 0 {
                        continue;
                    }

                    // 下载的 chunk 数
                    let current_count = self.get_chunks().await.len();
                    let diff = download_connection_count as i16 - current_count as i16;
                    // 有空闲连接数
                    if diff > 0 {
                        // 先置空，然后使用这些连接
                        self.superfluities_connection_count.store(0, Ordering::SeqCst);
                        for _ in 0..diff {
                            match download_next_chunk().await {
                                // 没有下一个 chunk
                                None => {
                                    is_iter_finished = true;
                                    break;
                                }
                                Some(future) => futures_unordered.push(future)
                            }
                        }
                    } else {
                        self.superfluities_connection_count.store(diff.unsigned_abs() as u8, Ordering::SeqCst);
                    }

                     futures_unordered.push(RunFuture::DownloadConnectionCountChanged(async move {
                         // 等待变更通知
                         let _ = receiver.changed().await;
                         // 取出值
                         let i = *receiver.borrow();
                         (receiver, i)
                     }));
                }
                // 下载完成
                RunFutureResult::ChunkDownloadEnd {
                    chunk_index,
                    result: Ok(DownloadingEndCause::DownloadFinished)
                } => {
                    let (downloading_chunk_count) = self.remove_chunk(chunk_index);

                    // breakpoint resume
                    // save_data().await;

                    // 迭代完成
                    if is_iter_finished {
                        if downloading_chunk_count == 0 {
                            break;
                        }
                    } else if self.superfluities_connection_count.load(Ordering::SeqCst) == 0 {
                        match download_next_chunk().await {
                            // 没有则退出
                            None => {
                                is_iter_finished = true;
                                if downloading_chunk_count == 0 {
                                    break;
                                }
                            }
                            // 还有则继续推送到队列
                            Some(future) => futures_unordered.push(future)
                        }
                    } else {
                        self.superfluities_connection_count.fetch_sub(1, Ordering::SeqCst);
                    }
                }
                // 取消下载
                RunFutureResult::ChunkDownloadEnd { result: Ok(DownloadingEndCause::Cancelled) , .. } => {
                    if matches!(result, Ok(DownloadingEndCause::Cancelled)) {
                        result = Ok(DownloadingEndCause::Cancelled);
                        // 取消监听 连接数 的更改
                        let _ = self.download_connection_count_sender.send(0);
                    }
                }
                // 下载出错
                RunFutureResult::ChunkDownloadEnd { result: Err(err), .. } => {
                    // 只记录第一个错误
                    if matches!(result,Ok(DownloadingEndCause::DownloadFinished)) {
                        result = Err(err);
                        // 取消监听 连接数 的更改
                        let _ = self.download_connection_count_sender.send(0);
                        // 取消其他的 Chunk 下载
                        self.cancel_token.cancel();
                    }
                }
                _ => {}
            }
        }

        if !matches!(result, Ok(DownloadingEndCause::DownloadFinished)) {
            // breakpoint resume -> save_data
        }

        result
    }

    /// 移除下载中的 chunk 记录，并返回还剩多少个 chunk 在下载
    async fn remove_chunk(&self, chunk_index: usize) -> (usize, Option<Arc<ChunkItem>>) {
        let mut downloading_chunks= self.downloading_chunks.lock().await;
        let removed = downloading_chunks.remove(&chunk_index);
        (downloading_chunks.len(), removed)
    }

    /// 获取所有下载中的 chunk，并根据开始的 range 排序
    pub async fn get_chunks(&self) -> Vec<Arc<ChunkItem>> {
        let mut downloading_chunks: Vec<_> = self
            .downloading_chunks
            .lock()
            .await
            .values()
            .cloned()
            .collect();

        downloading_chunks.sort_by(|a, b| {
            a.chunk_info.range.start.cmp(&b.chunk_info.range.start)
        });

        downloading_chunks
    }

    /// 获取连接数
    pub fn connection_count(&self) -> u8 {
        *self.download_connection_count_sender.borrow()
    }

    async fn insert_chunk(&self, item: Arc<ChunkItem>) {
        let mut downloading_chunks = self.downloading_chunks.lock().await;
        downloading_chunks.insert(item.chunk_info.index, item);
    }

    async fn download_next_chunk(
        &self,
        file: Arc<Mutex<File>>,
        request: Box<Request>
    ) -> Option<(usize, impl Future<Output = Result<DownloadingEndCause, DownloadError>>)> {
        if let Some(chunk_info) = self.chunk_iterator.next() {
            let chunk_item = Arc::new(ChunkItem::new(
                chunk_info,
                self.cancel_token.child_token(),
                file,
                self.client.clone(),
            ));

            self.insert_chunk(chunk_item.clone()).await;

            Some((
                chunk_item.chunk_info.index,
                chunk_item.download_chunk(request, self.retry_count)
            ))
        } else {
            // 没有 Chunk 了
            None
        }
    }
}
