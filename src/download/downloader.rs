use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use anyhow::Result;
use headers::HeaderMapExt;
use reqwest::Request;
use tokio::sync::{watch};
use url::Url;
use crate::download::chunk_manager::ChunkManager;
use crate::download::error::{DownloadEndCause, DownloadError, DownloadStartError};

type DownloadResult = Result<DownloadEndCause, DownloadError>;

pub struct DownloaderConfig {
    url: Url,
    save_dir: PathBuf,
    file_name: String,
    connection_count: u8,
}

impl DownloaderConfig {
    pub fn create_http_request(&self) -> Request {
        let mut request = Request::new(reqwest::Method::GET, self.url.clone());
        let header_map = request.headers_mut();

        // 浏览器代理
        // if config.use_browser_user_agent() {}

        header_map.insert(reqwest::header::ACCEPT, headers::HeaderValue::from_str("*/*").unwrap());
        header_map.typed_insert(headers::Connection::keep_alive());

        // timeout
        // request.timeout_mut();

        request
    }
}

pub struct Downloader {
    config: Arc<DownloaderConfig>,
    chunk_manager: Option<ChunkManager>,
    action_sender: watch::Sender<DownloadEndCause>,
    action_receiver: watch::Receiver<DownloadEndCause>
}

impl Downloader {
    pub fn new(config: DownloaderConfig) -> Self {
        let (tx, rx) = watch::channel(DownloadEndCause::Finished);

        Self {
            action_sender: tx,
            action_receiver: rx,
            chunk_manager: None,
            config: Arc::new(config),
        }
    }

    fn file_path(&self) -> PathBuf {
        self.config.save_dir.join(&self.config.file_name)
    }

    pub async fn download(&self) -> Result<DownloadResult, DownloadStartError>
    {
        if self.chunk_manager.is_some() {
            return Err(DownloadStartError::AlreadyDownloading);
        }

        if !self.config.save_dir.exists() {
           return Err(DownloadStartError::DirectoryDoesNotExist);
        }

        let res = self.run_download().await;

        Ok(res)
    }

    async fn run_download(&self) -> DownloadResult {
        Ok(DownloadEndCause::Finished)
        // async move {
        //     Ok(DownloadEndCause::Finished)
        // }
    }
}