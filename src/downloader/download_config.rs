use std::num::{NonZeroU8, NonZeroUsize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use headers::{HeaderMap, HeaderMapExt};
use reqwest::Request;
use tokio_util::sync::CancellationToken;
use url::Url;

type HttpRequestConfigureFutureFn = Box<dyn Fn(Request) -> Request + Send + Sync + 'static>;
type DownloadFileOpenOption = Box<dyn Fn(&mut std::fs::OpenOptions) + Send + Sync + 'static>;
/// 下载配置文件
pub struct HttpDownloadConfig {
    // 提前设置长度，如果存储空间不足将提前报错
    pub set_len_in_advance: bool,
    pub download_connection_count: NonZeroU8,
    pub chunk_size: NonZeroUsize,
    pub chunks_send_interval: Option<Duration>,
    pub save_dir: PathBuf,
    pub file_name: String,
    pub open_option: DownloadFileOpenOption,
    pub create_dir: bool,
    pub url: Arc<Url>,
    pub request_retry_count: u8,
    pub header_map: HeaderMap,
    pub downloaded_len_send_interval: Option<Duration>,
    pub strict_check_accept_ranges: bool,
    pub http_request_configure: Option<HttpRequestConfigureFutureFn>,
    pub cancel_token: Option<CancellationToken>,
    pub use_browser_use_agent: bool,
}

impl HttpDownloadConfig {
    pub fn file_path(&self) -> PathBuf {
        self.save_dir.join(&self.file_name)
    }

    pub fn create_http_request(&self) -> Request {
        let url = (*self.url).clone();
        let mut request = Request::new(reqwest::Method::GET, url);
        let header_map = request.headers_mut();

        // 设置 Agent
        if self.use_browser_use_agent {
            let agent = headers::HeaderValue::from_str("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36").unwrap();
            header_map.insert(reqwest::header::USER_AGENT, agent);
        }
        header_map.insert(reqwest::header::ACCEPT, headers::HeaderValue::from_str("*/*").unwrap());
        header_map.typed_insert(headers::Connection::keep_alive());
        for (header_name, header_value) in self.header_map.iter() {
            header_map.insert(header_name, header_value.clone());
        }

        match self.http_request_configure.as_ref() {
            None => request,
            Some(cfg) => cfg(request),
        }
    }
}

