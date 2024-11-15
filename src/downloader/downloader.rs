use std::future::Future;
use std::io::SeekFrom;
use std::num::{NonZeroU8, NonZeroUsize};
use std::path::PathBuf;
use std::sync::Arc;
use reqwest::Client;
use tokio_util::sync::CancellationToken;
use url::Url;
use anyhow::Result;
use headers::HeaderMapExt;
use parking_lot::RwLock;
use tokio::io::AsyncSeekExt;
use tokio::time::Instant;

use crate::downloader::chunk_iterator::{ChunkIterator, ChunkIteratorData, RemainingChunks};
use crate::downloader::chunk_manager::ChunkManger;
use crate::downloader::download_way::{DownloadSingle, DownloadWay};
use crate::downloader::error::{DownloadEndCause, DownloadError, DownloadStartError};

pub struct DownloadingState {
    pub download_instant: Instant,
    download_way: DownloadWay,
}

pub struct DownloaderConfig {
    chunk_size: NonZeroUsize,
    url: Url,
    save_dir: PathBuf,
    retry_count: u8,
    file_name: String,
    connection_count: NonZeroU8,
    cancel_token: Option<CancellationToken>
}

impl DownloaderConfig {
    pub fn file_path(&self) -> PathBuf {
        self.save_dir.join(&self.file_name)
    }

    pub fn create_http_request(&self) -> reqwest::Request {
        let url = self.url.clone();
        let mut request = reqwest::Request::new(reqwest::Method::GET, url);
        let header_map = request.headers_mut();
        header_map.insert(reqwest::header::ACCEPT, headers::HeaderValue::from_str("*/*").unwrap());
        header_map.typed_insert(headers::Connection::keep_alive());

        request
    }
}

pub struct FileDownloader {
    config: Arc<DownloaderConfig>,
    cancel_token: CancellationToken,
    client: Client,
    file_size: u64,
    downloading_state: Arc<RwLock<Option<Arc<DownloadingState>>>>,
}

impl FileDownloader {
    pub fn new(file_size: u64, config: DownloaderConfig) -> Self {
        let cancel_token = config.cancel_token.clone().unwrap_or_default();

        Self {
            cancel_token,
            file_size,
            config: Arc::new(config),
            client: Client::new(),
            downloading_state: Arc::new(RwLock::new(None))
        }
    }

    pub fn download(&mut self) -> Result<impl Future<Output=Result<DownloadEndCause, DownloadError>>, DownloadStartError> {
        if self.is_downloading() {
            return Err(DownloadStartError::AlreadyDownloading);
        }

        if !self.config.save_dir.exists() {
            return Err(DownloadStartError::DirectoryDoesNotExist);
        }

        Ok(self.start_download())
    }

    fn start_download(&mut self) -> impl Future<Output=Result<DownloadEndCause, DownloadError>> {
        if self.cancel_token.is_cancelled() {
            self.cancel_token = CancellationToken::new();
        }

        let client = self.client.clone();
        let file_size = self.file_size;
        let cancel_token = self.cancel_token.clone();
        let config = Arc::clone(&self.config);
        let downloading_state = self.downloading_state.clone();

        async move {
            let download_way = if file_size <= 1024 * 1024 * 10 {
                DownloadWay::Single(DownloadSingle {})
            } else {
                let chunk_data = ChunkIteratorData {
                    iter_count: 0,
                    remaining: RemainingChunks::new(config.chunk_size, file_size),
                };
                let chunk_iter = ChunkIterator::new(chunk_data);
                let chunk_manager = ChunkManger::new(
                    config.connection_count,
                    config.retry_count,
                    chunk_iter,
                    client,
                    cancel_token
                );
                DownloadWay::Range(chunk_manager)
            };

            let state = Arc::new(
                DownloadingState { download_instant: Instant::now(), download_way }
            );
            {
                let mut state_guard = downloading_state.write();
                *state_guard = Some(state.clone());
            }

            let file = {
                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .open(config.file_path())
                    .await
                    .map_err(|err| DownloadError::IOError(err))?;
                // 预设文件大小
                // ...
                file.seek(SeekFrom::Start(0)).await.map_err(|err| DownloadError::IOError(err))?;
                file
            };

            let res = match &state.download_way {
                DownloadWay::Single(_single_way) => {
                    Result::<DownloadEndCause, DownloadError>::Ok(DownloadEndCause::Finished)
                }
                DownloadWay::Range(chunk_manager) => {
                    chunk_manager.start_download(file, config.create_http_request()).await
                }
            };

            res
        }
    }

    fn is_downloading(&self) -> bool {
        self.downloading_state.read().is_some()
    }
}

mod tests {
    use std::num::{NonZeroU8, NonZeroUsize};
    use std::path::PathBuf;
    use tokio_util::sync::CancellationToken;
    use url::Url;
    use crate::downloader::downloader::{DownloaderConfig, FileDownloader};
    use crate::downloader::util::get_file_length;

    #[tokio::test]
    async fn should_be_run() {
        let file_url = Url::parse("http://localhost:23333/image.jpg").unwrap();
        let file_size = get_file_length(file_url.clone()).await.unwrap();
        let mut save_dir = PathBuf::new();
        save_dir.push("C:/Users/X/Downloads");
        let config = DownloaderConfig {
            chunk_size: NonZeroUsize::new(1024 * 1024 * 4).unwrap(),
            url: file_url,
            save_dir,
            retry_count: 3,
            file_name: "demo.jpg".to_string(),
            connection_count: NonZeroU8::new(3).unwrap(),
            cancel_token: Some(CancellationToken::new()),
        };
        let mut downloader = FileDownloader::new(file_size, config);

        match downloader.download() {
            Ok(result) => {
                match result.await {
                    Ok(end_cause) => {
                        println!("Success: {:?}", end_cause);
                    }
                    Err(err) => {
                        println!("Inner: {:?}", err);
                    }
                }
            },
            Err(err) => {
                println!("Wrapper: {:?}", err);
            }
        }
    }
}
