use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use parking_lot::RwLock;
use tokio::sync::watch;
use tokio::time::Instant;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use crate::downloader::download_config::HttpDownloadConfig;
use crate::downloader::download_way::DownloadWay;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
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
    client: reqwest::Client,
    total_size_semaphore: Arc<tokio::sync::Semaphore>,

    pub downloaded_len_receiver: watch::Receiver<u64>,
    downloaded_len_sender: Arc<watch::Sender<u64>>,
}

impl HttpFileDownloader {

}
