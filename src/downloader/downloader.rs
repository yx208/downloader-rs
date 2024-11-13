use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

Ex

use futures_util::{FutureExt, Stream};
use parking_lot::RwLock;
use reqwest::{Client, Error, Response};
use tokio::sync::watch;
use tokio::time::Instant;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use anyhow::Result;
use futures_util::future::BoxFuture;
use headers::HeaderMapExt;
use thiserror::Error;

use crate::downloader::download_config::HttpDownloadConfig;
use crate::downloader::download_way::DownloadWay;

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

    #[error("IoError，{:?}", .0)]
    IOError(#[from] tokio::io::Error),

    #[error("http request failed, {:?}", .0)]
    HttpRequestFailed(#[from] reqwest::Error),

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
pub struct HttpFileDownloader {
    pub config: Arc<HttpDownloadConfig>,
    pub content_length: Arc<AtomicU64>,
    pub cancel_token: CancellationToken,
    pub archive_data_future: Option<Excl>
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
            content_length: Default::default(),
            downloading_state: Default::default(),
            downloaded_len_sender: Arc::new(tx),
            downloaded_len_receiver: rx,
        }
    }

    pub fn is_downloading(&self) -> bool {
        self.downloading_state.read().is_some()
    }

    // pub fn downloaded_len_stream(&self) -> impl Stream<Item = u64> + 'static {
    //     let mut downloaded_len_receiver = self.downloaded_len_receiver.clone();
    //     let duration = self.config.downloaded_len_send_interval.clone();
    //
    //     // 定义异步流
    //     async_stream::stream! {
    //         let downloaded_len = *downloaded_len_receiver.borrow();
    //         yield downloaded_len;
    //
    //         while downloaded_len_receiver.changed().await.is_ok() {
    //             let downloaded_len = *downloaded_len_receiver.borrow();
    //             yield downloaded_len;
    //
    //             if let Some(duration) = duration {
    //                 tokio::time::sleep(duration).await;
    //             }
    //         }
    //     }
    // }

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

        let config = Arc::clone(&self.config);
        let client = self.client.clone();
        let total_size_semaphore = Arc::clone(&self.total_size_semaphore);
        let content_length_arc = Arc::clone(&self.content_length);
        let archive_data_future = self.

        async move {
            fn request<'a>(client: &'a Client, config: &'a HttpDownloadConfig)
                -> BoxFuture<'a, Result<Response, DownloadError>>
            {
                async move {
                    let mut retry_count = 0;
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

                let is_ranges_bytes_none = accept_ranges.is_none();
                let is_ranges_bytes = !is_ranges_bytes_none && accept_ranges.unwrap() == headers::AcceptRanges::bytes();
                let archive_data = match  {  };

                DownloadingEndCause::DownloadFinished
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
