use std::num::{NonZeroU8, NonZeroUsize};
use std::path::PathBuf;
use std::sync::Arc;
use url::Url;
use crate::downloader::download_config::HttpDownloadConfig;
use crate::downloader::downloader::HttpFileDownloader;

mod downloader;
mod extension;

#[tokio::main]
async fn main() {

    let url = "https://tasset.xgy.tv/down/resources/agency/CeShiJiGou_1/DingJiBuMen_1/dc9152119c160a601e5f684795fb4ea2_16/20241114/11362144b5cf69bb0fc4699504.mp4";
    let client = reqwest::Client::new();
    let config = HttpDownloadConfig {
        set_len_in_advance: true,
        download_connection_count: NonZeroU8::new(3).unwrap(),
        chunk_size: NonZeroUsize::new(1024 * 1024 * 4).unwrap(),
        chunks_send_interval: None,
        save_dir: PathBuf::from("C:/Users/User/Downloads"),
        file_name: "文西.mp4".to_string(),
        create_dir: false,
        url: Arc::new(Url::parse(&url).unwrap()),
        request_retry_count: 3,
        header_map: Default::default(),
        downloaded_len_send_interval: None,
        strict_check_accept_ranges: false,
        http_request_configure: None,
        cancel_token: None,
        use_browser_use_agent: true,
    };

    let mut downloader = HttpFileDownloader::new(client, Arc::new(config));

    match downloader.download() {
        Ok(future) => {
            match future.await {
                Ok(res) => {
                    println!("Downloaded to {:?}", res);
                }
                Err(_) => {}
            }
        }
        Err(_) => {}
    };
}
