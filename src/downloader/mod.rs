pub mod chunk_item;
pub mod chunk_iterator;
pub mod chunk_manager;
pub mod downloader;
pub mod download_config;
pub mod download_way;
pub mod exclusive;

use std::borrow::Cow;
use std::num::{NonZeroU8, NonZeroUsize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use headers::HeaderMap;
use reqwest::Request;
use tokio_util::sync::CancellationToken;
use url::Url;
use crate::downloader::chunk_iterator::ChunkData;
use crate::downloader::download_config::HttpDownloadConfig;
use crate::downloader::downloader::{ExtendedHttpFileDownloader, HttpFileDownloader};

#[derive(Debug, Clone)]
pub struct DownloadArchiveData {
    pub downloaded_len: u64,
    pub downloading_duration: u32,
    pub chunk_data: Option<ChunkData>
}

pub struct HttpDownloaderBuilder {
    chunk_size: NonZeroUsize,
    download_connection_count: NonZeroU8,
    url: Url,
    save_dir: PathBuf,
    set_len_in_advance: bool,
    file_name: Option<String>,
    open_option: Box<dyn Fn(&mut std::fs::OpenOptions) + Send + Sync + 'static>,
    create_dir: bool,
    request_retry_count: u8,
    client: Option<reqwest::Client>,
    header_map: HeaderMap,
    /// 更新已下载长度的发送间隔
    downloaded_len_send_interval: Option<Duration>,
    strict_check_accept_ranges: bool,
    http_request_configure: Option<Box<dyn Fn(Request) -> Request + Send + Sync + 'static>>,
    cancel_token: Option<CancellationToken>,
    use_browser_user_agent: bool,
}

impl HttpDownloaderBuilder {
    pub fn new(url: Url, save_dir: PathBuf) -> Self {
        Self {
            url,
            save_dir,
            client: None,
            create_dir: true,
            strict_check_accept_ranges: true,
            set_len_in_advance: false,
            use_browser_user_agent: true,
            request_retry_count: 3,
            // 4M
            chunk_size: NonZeroUsize::new(1024 * 1024 * 4).unwrap(),
            download_connection_count: NonZeroU8::new(3).unwrap(),
            file_name: None,
            http_request_configure: None,
            cancel_token: None,
            open_option: Box::new(|opt| {
                opt.create(true).write(true);
            }),
            header_map: Default::default(),
            downloaded_len_send_interval: Some(Duration::from_millis(300)),
        }
    }

    pub fn set_client(mut self, client: Option<reqwest::Client>) -> Self {
        self.client = client;
        self
    }

    pub fn set_cancel_token(mut self, cancel_token: Option<CancellationToken>) -> Self {
        self.cancel_token = cancel_token;
        self
    }

    pub fn build(self) -> ExtendedHttpFileDownloader {
        let mut downlaoder = HttpFileDownloader::new(
            self.client.unwrap_or(Default::default()),
            Arc::new(HttpDownloadConfig {
                set_len_in_advance: self.set_len_in_advance,
                download_connection_count: self.download_connection_count,
                chunk_size: self.chunk_size,
                chunks_send_interval: None,
                save_dir: self.save_dir,
                file_name: self.file_name.unwrap_or_else(|| self.url.file_name().to_string()),
                open_option: self.open_option,
                create_dir: self.create_dir,
                url: Arc::new(self.url),
                request_retry_count: self.request_retry_count,
                header_map: self.header_map,
                downloaded_len_send_interval: self.downloaded_len_send_interval,
                strict_check_accept_ranges: self.strict_check_accept_ranges,
                http_request_configure: self.http_request_configure,
                cancel_token: self.cancel_token,
                use_browser_use_agent: self.use_browser_user_agent,
            })
        );

        ExtendedHttpFileDownloader::new(downlaoder)
    }
}


/// 为 url 这个 struct 添加一个获取 filename 的方法
pub trait UrlFileName {
    fn file_name(&self) -> Cow<str>;
}

impl UrlFileName for Url {
    fn file_name(&self) -> Cow<str> {
        let website_default: &'static str = "index.html";
        self.path_segments()
            .map(|n| {
                n.last()
                    .map(|n| Cow::Borrowed(if n.is_empty() { website_default } else { n }))
                    .unwrap_or_else(|| {
                        self.domain()
                            .map(Cow::Borrowed)
                            .unwrap_or(Cow::Owned(website_default.to_string()))
                    })
            })
            .unwrap_or_else(|| Cow::Owned(website_default.to_string()))
    }
}
