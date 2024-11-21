use std::future::Future;
use std::num::{NonZeroU8, NonZeroUsize};
use std::path::PathBuf;
use std::sync::Arc;
use anyhow::Result;
use futures_util::{Stream, StreamExt};
use headers::HeaderMapExt;
use reqwest::Request;
use tokio::fs::OpenOptions;
use tokio::sync::{watch};
use tokio::task::JoinHandle;
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
    // 发送接收 pause/cancel 指令
    action_sender: watch::Sender<DownloadActionNotify>,
    action_receiver: watch::Receiver<DownloadActionNotify>,
    // 发送接收数据接收长度
    downloaded_len_sender: watch::Sender<u64>,
    downloaded_len_receiver: watch::Receiver<u64>,

    listen_len_change_handle: Option<JoinHandle<()>>
}

impl Downloader {
    pub fn new(config: DownloaderConfig) -> Self {
        let (
            action_sender,
            action_receiver
        ) = watch::channel(DownloadActionNotify::Notify(DownloadEndCause::Finished));
        let (
            downloaded_len_sender,
            downloaded_len_receiver
        ) = watch::channel(0);

        Self {
            action_sender,
            action_receiver,
            downloaded_len_sender,
            downloaded_len_receiver,
            chunk_manager: None,
            listen_len_change_handle: None,
            config: Arc::new(config),
        }
    }

    fn file_path(&self) -> PathBuf {
        self.config.save_dir.join(&self.config.file_name)
    }

    pub fn downloaded_len_stream(&self) -> impl Stream<Item=u64> + 'static {
        let mut receiver = self.downloaded_len_receiver.clone();

        async_stream::stream! {
            let len = *receiver.borrow();
            yield len;

            while receiver.changed().await.is_ok() {
                let len = *receiver.borrow();
                yield len;
            }
        }
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
        let chunk_manager = Arc::new(ChunkManager::new(
            self.config.connection_count.get(),
            iter,
            self.action_sender.clone(),
            self.action_receiver.clone()
        ));

        self.chunk_manager = Some(chunk_manager.clone());

        let file_path = self.file_path();
        let config = self.config.clone();
        let len_sender = self.downloaded_len_sender.clone();

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
                len_sender.clone()
            ).await;

            result
        }
    }

    fn is_downloading(&self) -> bool {
        self.chunk_manager.is_some()
    }

    pub fn pause(&mut self) {
        match self.action_sender.send(DownloadActionNotify::Notify(DownloadEndCause::Paused)) {
            Ok(_) => {
                self.chunk_manager.take();
            }
            Err(_) => {}
        }
    }

    pub fn cancel(&mut self) {
        match self.action_sender.send(DownloadActionNotify::Notify(DownloadEndCause::Cancelled)) {
            Ok(_) => {
                self.chunk_manager.take();
            }
            Err(_) => {}
        }
    }
}

mod tests {
    use tokio::sync::Mutex;
    use super::*;

    async fn create_downloader() -> Downloader {
        let download_dir = dirs::download_dir().unwrap();
        let config = DownloaderConfig {
            retry_count: 3,
            url: Url::parse("https://tasset.xgy.tv/down/resources/agency/CeShiJiGou_1/HouDuanBuMen_3/dc9152119c160a601e5f684795fb4ea2_16/20241107/150942e889862aa83534036990.mkv").unwrap(),
            save_dir: download_dir,
            file_name: "demo.mkv".to_string(),
            chunk_size: NonZeroUsize::new(1024 * 1024 * 4).unwrap(),
            connection_count: NonZeroU8::new(3).unwrap(),
        };
        let downloader = Downloader::new(config);

        downloader
    }

    #[tokio::test]
    async fn should_be_pause() {
        let downloader = Arc::new(Mutex::new(create_downloader().await));
    }

    #[tokio::test]
    async fn should_be_cancel() {
        let downloader = create_downloader().await;
        let downloader = Arc::new(Mutex::new(downloader));

        let downloader_clone = downloader.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            let mut guard = downloader_clone.lock().await;
            guard.cancel();
        });

        let future = {
            let mut guard = downloader.lock().await;
            guard.download().await
        };

        match future {
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

    #[tokio::test]
    async fn should_be_download() {
        let mut downloader = create_downloader().await;

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

