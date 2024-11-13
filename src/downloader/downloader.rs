use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use futures_util::Stream;
use parking_lot::RwLock;
use reqwest::Client;
use tokio::sync::watch;
use tokio::time::Instant;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use crate::downloader::download_config::HttpDownloadConfig;
use crate::downloader::download_way::DownloadWay;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("{:?}", .0)]
    Other(#[from] anyhow::Error),
    #[error("IoError，{:?}", .0)]
    IOError(#[from] tokio::io::Error),
    #[error("http request failed, {:?}", .0)]
    HttpRequestFailed(#[from] reqwest::Error)
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

    pub fn downloaded_len_stream(&self) -> impl Stream<Item = u64> + 'static {
        let mut downloaded_len_receiver = self.downloaded_len_receiver.clone();
        let duration = self.config.downloaded_len_send_interval.clone();

        // 定义异步流
        async_stream::stream! {
            let downloaded_len = *downloaded_len_receiver.borrow();
            yield downloaded_len;

            while downloaded_len_receiver.changed().await.is_ok() {

            }
        }
    }
}
