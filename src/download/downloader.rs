use std::future::Future;
use std::num::{NonZeroU8, NonZeroUsize};
use std::path::PathBuf;
use std::sync::Arc;
use anyhow::Result;
use headers::HeaderMapExt;
use reqwest::Request;
use tokio::fs::OpenOptions;
use tokio::sync::{watch};
use url::Url;
use crate::download::chunk_manager::ChunkManager;
use crate::download::chunk_range::{ChunkData, ChunkRangeIterator};
use crate::download::error::{DownloadActionNotify, DownloadEndCause, DownloadError, DownloadStartError};
use crate::download::util::get_file_length;

type DownloadResult = Result<DownloadEndCause, DownloadError>;

pub struct DownloaderConfig {
    url: Url,
    save_dir: PathBuf,
    file_name: String,
    chunk_size: NonZeroUsize,
    connection_count: NonZeroU8,
    retry_count: u8,
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

    pub fn file_path(&self) -> PathBuf {
        self.save_dir.join(&self.file_name)
    }
}

pub struct Downloader {
    config: Arc<DownloaderConfig>,
    chunk_manager: Option<Arc<ChunkManager>>,
    action_sender: watch::Sender<DownloadActionNotify>,
    action_receiver: watch::Receiver<DownloadActionNotify>
}

impl Downloader {
    pub fn new(config: DownloaderConfig) -> Self {
        let (tx, rx) = watch::channel(DownloadActionNotify::Notify(DownloadEndCause::Finished));

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

    pub async fn download(&mut self) -> Result<impl Future<Output=DownloadResult>, DownloadStartError> {
        if self.chunk_manager.is_some() {
            return Err(DownloadStartError::AlreadyDownloading);
        }

        if !self.config.save_dir.exists() {
           return Err(DownloadStartError::DirectoryDoesNotExist);
        }

        Ok(self.run_download().await)
    }

    async fn run_download(&mut self) -> impl Future<Output=DownloadResult> {
        let content_length = get_file_length(self.config.url.clone()).await.unwrap();
        let chunk_data = ChunkData::new(self.config.chunk_size.clone(), content_length);
        let iter = ChunkRangeIterator::new(chunk_data);
        let chunk_manager = Arc::new(ChunkManager::new(self.config.connection_count.get(), iter));
        let sender = self.action_sender.clone();
        let receiver = self.action_receiver.clone();

        self.chunk_manager = Some(chunk_manager.clone());

        let file_path = self.file_path();
        let config = self.config.clone();

        async move {
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .open(file_path)
                .await?;

            let result = chunk_manager.download(
                file,
                config.create_http_request(),
                config.retry_count,
                receiver,
                sender
            ).await;

            result
        }
    }

    fn is_downloading(&self) -> bool {
        self.chunk_manager.is_some()
    }

    pub fn pause() {

    }

    pub fn cancel() {

    }
}

mod tests {
    use super::*;

    #[tokio::test]
    async fn should_be_download() {
        let download_dir = dirs::download_dir().unwrap();
        let config = DownloaderConfig {
            retry_count: 3,
            url: Url::parse("http://localhost:23333/image.jpg").unwrap(),
            save_dir: download_dir,
            file_name: "demo.jpg".to_string(),
            chunk_size: NonZeroUsize::new(1024 * 1024 * 4).unwrap(),
            connection_count: NonZeroU8::new(3).unwrap(),
        };
        let mut downloader = Downloader::new(config);

        match downloader.download().await {
            Ok(future) => {
                match future.await {
                    Ok(download_end_cause) => {
                        println!("Successful: {:?}", download_end_cause);
                    }
                    Err(err) => {
                        println!("Download error: {}", err);
                    }
                }
            }
            Err(err) => {
                println!("Start error: {}", err);
            }
        }
    }
}

