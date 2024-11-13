use std::future::Future;
use std::io::SeekFrom;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::{FutureExt};
use parking_lot::RwLock;
use reqwest::{Client, Response};
use tokio::sync::watch;
use tokio::time::Instant;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use anyhow::Result;
use futures_util::future::BoxFuture;
use headers::HeaderMapExt;
use thiserror::Error;
use tokio::io::AsyncSeekExt;
use crate::downloader::chunk_iterator::{ChunkData, ChunkIterator, RemainingChunks};
use crate::downloader::chunk_manager::ChunkManager;
use crate::downloader::download_config::HttpDownloadConfig;
use crate::downloader::download_way::{DownloadWay, SingleDownload};
use crate::downloader::DownloadArchiveData;
use crate::downloader::exclusive::Exclusive;

#[derive(Debug, Error)]
pub enum DownloadStartError {
    #[error("File create failed，{:?}", .0)]
    FileCreateFailed(#[from] tokio::io::Error),

    #[error("Already downloading")]
    AlreadyDownloading,

    #[error("Directory does not exist")]
    DirectoryDoesNotExist,

    #[error("Initializing")]
    Initializing,

    #[error("Starting")]
    Starting,

    #[error("Stopping")]
    Stopping
}

/// 响应无效原因
#[derive(Debug, Copy, Clone)]
pub enum HttpResponseInvalidCause {
    /// 长度无效
    ContentLengthInvalid,
    /// 状态码不成功
    StatusCodeUnsuccessful,
}

/// 下载错误
#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("{:?}", .0)]
    Other(#[from] anyhow::Error),

    #[error("ArchiveDataLoadError {:?}", .0)]
    ArchiveDataLoadError(anyhow::Error),

    #[error("IoError，{:?}", .0)]
    IOError(#[from] tokio::io::Error),

    #[error("Http request failed, {:?}", .0)]
    HttpRequestFailed(#[from] reqwest::Error),

    #[error("Http request response invalid，{:?}", .0)]
    HttpRequestResponseInvalid(HttpResponseInvalidCause, Response)
}

/// 下载结束原因
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum DownloadingEndCause {
    DownloadFinished,
    Cancelled,
    Paused
}

struct DownloadingState {
    pub downloading_duration: u32,
    pub download_instant: Instant,
    pub download_way: DownloadWay,
}

type DownloadingStateTyped = (oneshot::Receiver<DownloadingEndCause>, Arc<DownloadingState>);
type ArchiveDataFutureTyped = BoxFuture<'static, Result<Option<Box<DownloadArchiveData>>>>;
pub struct HttpFileDownloader {
    pub config: Arc<HttpDownloadConfig>,
    pub content_length: Arc<AtomicU64>,
    pub cancel_token: CancellationToken,
    pub archive_data_future: Option<Exclusive<ArchiveDataFutureTyped>>,
    /// oneshot: 一次性通道用于在异步任务之间发送单个消息。通道函数用于创建形成通道的发送方和接收方句柄对
    pub downloading_state_oneshot_vec: Vec<oneshot::Sender<Arc<DownloadingState>>>,
    downloading_state: Arc<RwLock<Option<DownloadingStateTyped>>>,
    client: Client,
    total_size_semaphore: Arc<tokio::sync::Semaphore>,

    pub downloaded_len_receiver: watch::Receiver<u64>,
    downloaded_len_sender: Arc<watch::Sender<u64>>,
}

impl HttpFileDownloader {
    pub fn new(client: Client, config: Arc<HttpDownloadConfig>) -> Self {
        let cancel_token = config.cancel_token.clone().unwrap_or_default();
        let (tx, rx) = watch::channel::<u64>(0);
        let total_size_semaphore = Arc::new(tokio::sync::Semaphore::new(0));

        Self {
            config,
            total_size_semaphore,
            client,
            cancel_token,
            archive_data_future: None,
            downloading_state_oneshot_vec: vec![],
            content_length: Default::default(),
            downloading_state: Default::default(),
            downloaded_len_sender: Arc::new(tx),
            downloaded_len_receiver: rx,
        }
    }

    pub fn is_downloading(&self) -> bool {
        self.downloading_state.read().is_some()
    }

    fn reset(&self) {
        // 重置长度
        self.downloaded_len_sender.send(0).unwrap_or_else(|_| {
            // Written log
        });
    }

    pub fn download(&mut self) -> Result<
        impl Future<Output = Result<DownloadingEndCause, DownloadError>> + Send + 'static,
        DownloadStartError
    > {
        self.reset();

        if self.is_downloading() {
            return Err(DownloadStartError::AlreadyDownloading);
        }

        if self.config.create_dir {
            // 递归创建目录
            std::fs::create_dir_all(&self.config.save_dir)?;
        } else if !self.config.save_dir.exists() {
            return Err(DownloadStartError::DirectoryDoesNotExist);
        }

        Ok(self.start_download())
    }

    fn start_download(&mut self) -> impl Future<Output = Result<DownloadingEndCause, DownloadError>> + Send + 'static {
        if self.cancel_token.is_cancelled() {
            self.cancel_token = CancellationToken::new();
        }

        let client = self.client.clone();
        let config = Arc::clone(&self.config);
        let total_size_semaphore = Arc::clone(&self.total_size_semaphore);
        let content_length_arc = Arc::clone(&self.content_length);
        let downloading_state = Arc::clone(&self.downloading_state);
        let downloaded_len_sender = Arc::clone(&self.downloaded_len_sender);
        let archive_data_future = self.archive_data_future.take();
        let cancel_token = self.cancel_token.clone();
        let downloading_state_oneshot_vec = self.downloading_state_oneshot_vec
            .drain(..)
            .collect::<Vec<oneshot::Sender<Arc<DownloadingState>>>>();

        async move {
            fn request<'a>(client: &'a Client, config: &'a HttpDownloadConfig)
                -> BoxFuture<'a, Result<Response, DownloadError>>
            {
                async move {
                    let mut retry_count = 0;
                    // 执行请求 chunk
                    let response = loop {
                        let res = client
                            .execute(config.create_http_request())
                            .await
                            .and_then(|n| n.error_for_status());

                        if res.is_err() && retry_count < config.request_retry_count {
                            retry_count += 1;
                            continue;
                        }

                        break res;
                    };

                    match response {
                        // 请求是成功的，但返回的状态码异常
                        Ok(res) if !res.status().is_success() => {
                            Err(
                                DownloadError::HttpRequestResponseInvalid(
                                    HttpResponseInvalidCause::StatusCodeUnsuccessful,
                                    res
                                )
                            )
                        }
                        // 正常成功
                        Ok(res) => Ok(res),
                        // 请求失败
                        Err(err) => Err(DownloadError::HttpRequestFailed(err))
                    }
                }.boxed()
            }

            let (end_sender, end_receiver) = oneshot::channel();
            let download_end_cause = {
                // 响应结果
                let response = match request(&client, &config).await {
                    Ok(res) => res,
                    Err(err) => {
                        total_size_semaphore.add_permits(1);
                        return Err(err.into());
                    }
                };

                // 获取响应内容长度
                let content_length = response
                    .headers()
                    .typed_get::<headers::ContentLength>()
                    .map(|v| v.0)
                    .and_then(|v| {
                        if v == 0 {
                            None
                        } else {
                            Some(v)
                        }
                    });

                // 无效内容长度的
                if let Some(0) = content_length {
                    total_size_semaphore.add_permits(1);
                    return Err(
                        DownloadError::HttpRequestResponseInvalid(
                            HttpResponseInvalidCause::ContentLengthInvalid,
                            response
                        )
                    );
                }

                // 保存长度
                content_length_arc.store(content_length.unwrap_or(0), Ordering::Relaxed);

                // 允许内容范围
                let accept_ranges = response.headers().typed_get::<headers::AcceptRanges>();

                // 没有 accept-range 头
                let is_ranges_bytes_none = accept_ranges.is_none();
                // 是否为：`Accept-Ranges: bytes`
                let is_ranges_bytes = !is_ranges_bytes_none && accept_ranges.unwrap() == headers::AcceptRanges::bytes();
                let archive_data = match archive_data_future {
                    None => None,
                    Some(archive_data_future) => {
                        archive_data_future.await.map_err(DownloadError::ArchiveDataLoadError)?
                    }
                };
                let downloading_duration = archive_data
                    .as_ref()
                    .map(|value| value.downloading_duration)
                    .unwrap_or(0);

                // 获取下载方法
                let download_way = {
                    let has_content_length = {
                        if content_length.is_some() {
                            if config.strict_check_accept_ranges {
                                is_ranges_bytes
                            } else {
                                is_ranges_bytes_none || is_ranges_bytes
                            }
                        } else {
                            false
                        }
                    };

                    if has_content_length {
                        // 文件大小
                        let content_length = content_length.unwrap();
                        // chunk
                        let chunk_data = archive_data
                            // 如果不为 None
                            .and_then(|archive_data| {
                                // 通知已下载到了多大的数据
                                downloaded_len_sender
                                    .send(archive_data.downloaded_len)
                                    .unwrap_or_else(|_| { /* tracing */ });
                                archive_data.chunk_data.map(|mut data| {
                                    data.remaining.chunk_size = config.chunk_size.get();
                                    data
                                })
                            })
                            // None -> 没有归档数据
                            .unwrap_or_else(|| ChunkData {
                                iter_count: 0,
                                remaining: RemainingChunks::new(config.chunk_size, content_length),
                                last_incomplete_chunks: Default::default()
                            });

                        let chunk_iterator = ChunkIterator::new(content_length, chunk_data);
                        let chunk_manager = Arc::new(ChunkManager::new(
                            config.download_connection_count,
                            client,
                            cancel_token,
                            chunk_iterator,
                            config.request_retry_count
                        ));

                        DownloadWay::Ranges(chunk_manager)
                    } else {
                        DownloadWay::Single(SingleDownload::new())
                    }
                };

                // 下载中的状态
                let state = DownloadingState {
                    // 过去了多久
                    downloading_duration,
                    // 当前时间
                    download_instant: Instant::now(),
                    download_way
                };
                let state = Arc::new(state);
                {
                    // 重写 state
                    let mut state_guard = downloading_state.write();
                    *state_guard = Some((end_receiver, state.clone()))
                }

                total_size_semaphore.add_permits(1);

                // 创建文件
                let file = {
                    let mut options = std::fs::OpenOptions::new();
                    options.create(true).write(true);

                    let mut file = tokio::fs::OpenOptions::from(options)
                        .open(config.file_path())
                        .await?;
                    if config.set_len_in_advance {
                        file.set_len(content_length.unwrap()).await?;
                    }
                    file.seek(SeekFrom::Start(0)).await?;
                    file
                };

                // 通知所有目标变更状态
                for oneshot in downloading_state_oneshot_vec {
                    oneshot.send(state.clone()).unwrap_or_else(|_| {
                        // tracing
                    });
                }

                let download_end_cause_result = match &state.download_way {
                    DownloadWay::Ranges(item) => {
                        let request = Box::new(config.create_http_request());
                        item.start_download(file, request).await
                    }
                    DownloadWay::Single(item) => {
                        item.download(file, Box::new(response), config.chunk_size.get()).await
                    }
                };

                if {
                    // 在 block 写可以避免资源竞争，尽快释放锁
                    downloading_state.read().is_some()
                } {
                    let mut guard = downloading_state.write();
                    *guard = None;
                }

                download_end_cause_result?
            };

            end_sender.send(download_end_cause).unwrap_or_else(|_| {
                // tracing
            });

            Ok(download_end_cause)
        }
    }
}

type DownloadFuture = BoxFuture<'static, Result<DownloadingEndCause, DownloadError>>;

pub struct ExtendedHttpFileDownloader {
    pub inner: HttpFileDownloader,
}

impl ExtendedHttpFileDownloader {
    pub fn new(downloader: HttpFileDownloader) -> Self {
        Self {
            inner: downloader,
        }
    }

    pub fn prepare_download(&mut self) -> Result<DownloadFuture> {
        todo!()
    }
}
