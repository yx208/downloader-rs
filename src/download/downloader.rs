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
use uuid::Uuid;
use crate::download::archiver::Archiver;
use crate::download::chunk_manager::ChunkManager;
use crate::download::chunk_range::{ChunkData, ChunkRangeIterator};
use crate::download::error::{DownloadActionNotify, DownloadEndCause, DownloadError, DownloadStartError};
use crate::download::util::get_file_length;

type DownloadResult = Result<DownloadEndCause, DownloadError>;

pub struct DownloaderConfig {
    pub url: Url,
    pub save_dir: PathBuf,
    pub file_name: String,
    pub chunk_size: NonZeroUsize,
    pub concurrent: NonZeroU8,
    pub retry_count: u8,
}

impl DownloaderConfig {
    /// 只提供基础信息进行创建
    pub fn from_url(url: Url) -> Self {
        let file_name = if let Some(segments) = url.path_segments() {
            segments.last().unwrap().to_string()
        } else {
            Uuid::new_v4().to_string()
        };

        Self {
            url,
            file_name,
            retry_count: 3,
            save_dir: dirs::download_dir().unwrap(),
            chunk_size: NonZeroUsize::new(1024 * 1024 * 4).unwrap(),
            concurrent: NonZeroU8::new(3).unwrap(),
        }
    }

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
    chunk_manager: Option<Arc<ChunkManager>>,
    // 发送接收 pause/cancel 指令
    action_sender: Option<watch::Sender<DownloadActionNotify>>,
    action_receiver: Option<watch::Receiver<DownloadActionNotify>>,
    // 发送接收数据接收长度
    downloaded_len_sender: watch::Sender<u64>,
    downloaded_len_receiver: watch::Receiver<u64>,

    pub content_length: Option<u64>,
    listen_len_change_handle: Option<JoinHandle<()>>,
    archiver: Option<Archiver>
}

impl Downloader {
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(0);

        Self {
            action_sender: None,
            action_receiver: None,
            chunk_manager: None,
            listen_len_change_handle: None,
            content_length: None,
            archiver: None,
            downloaded_len_sender: tx,
            downloaded_len_receiver: rx,
        }
    }

    /// 拷贝一份 chunk 下载数据
    pub fn clone_chunk_data(&self) -> Option<ChunkData> {
        if let Some(chunk_manager) = &self.chunk_manager {
            let data = chunk_manager.chunk_iter.data.read();
            Some(data.clone())
        } else {
            None
        }
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

    pub async fn download(&mut self, config: Arc<DownloaderConfig>, archiver: Option<&Archiver>)
        -> Result<impl Future<Output=DownloadResult>, DownloadStartError>
    {
        if self.chunk_manager.is_some() {
            return Err(DownloadStartError::AlreadyDownloading);
        }

        if !config.save_dir.exists() {
           return Err(DownloadStartError::DirectoryDoesNotExist);
        }

        // 重置 watch 状态
        let (tx, rx) = watch::channel(DownloadActionNotify::Notify(DownloadEndCause::Finished));
        self.action_sender = Some(tx.clone());
        self.action_receiver = Some(rx.clone());

        let file_path = config.file_path();
        let len_sender = self.downloaded_len_sender.clone();
        let content_length = get_file_length(config.url.clone()).await.unwrap();

        let chunk_data = ChunkData::new(config.chunk_size.clone(), content_length);
        let iter = ChunkRangeIterator::new(chunk_data);
        let chunk_manager = Arc::new(ChunkManager::new(config.concurrent.get(), iter, tx, rx));

        self.chunk_manager = Some(chunk_manager.clone());
        self.content_length = Some(content_length);

        let future = async move {
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
        };

        Ok(future)
    }

    fn is_downloading(&self) -> bool {
        self.chunk_manager.is_some()
    }

    pub fn exec(&mut self, action: DownloadEndCause) -> Result<Arc<ChunkManager>, ()> {
        if let Some(sender) = self.action_sender.take() {
            sender.send(DownloadActionNotify::Notify(action)).unwrap();
            match self.chunk_manager.take() {
                None => Err(()),
                Some(r) => Ok(r)
            }
        } else {
            Err(())
        }
    }
}

mod tests {
    use super::*;
    use tokio::sync::Mutex;

    fn creat_config() -> DownloaderConfig {
        DownloaderConfig {
            retry_count: 3,
            url: Url::parse("https://tasset.xgy.tv/down/resources/agency/CeShiJiGou_1/QianDuanBuMen_2/dc9152119c160a601e5f684795fb4ea2_16/20241118/18154964fcaafce36522519013.mp4").unwrap(),
            save_dir: dirs::download_dir().unwrap(),
            file_name: "demo.mp4".to_string(),
            chunk_size: NonZeroUsize::new(1024 * 1024 * 4).unwrap(),
            concurrent: NonZeroU8::new(3).unwrap(),
        }
    }

    #[tokio::test]
    async fn should_be_pause() -> Result<()> {
        Ok(())
    }

    #[tokio::test]
    async fn should_be_cancel() {
        let downloader = Arc::new(Mutex::new(Downloader::new()));
        let config = Arc::new(creat_config());

        let downloader_clone = downloader.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            let mut guard = downloader_clone.lock().await;
            guard.cancel();
        });

        let future = {
            let mut guard = downloader.lock().await;
            guard.download(config.clone(), None).await
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
        let mut downloader = Downloader::new();
        let config = Arc::new(creat_config());

        match downloader.download(config, None).await {
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

