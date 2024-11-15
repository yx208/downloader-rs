use reqwest::{Client, Request};
use url::Url;
use crate::downloader::error::DownloadError;

pub async fn get_file_length(url: Url) -> anyhow::Result<Option<u64>, DownloadError> {
    let request = Request::new(reqwest::Method::HEAD, url);
    let client = Client::new();

    match client.execute(request).await {
        Ok(response) => {
            if let Some(content_length) = response.headers().get(reqwest::header::CONTENT_LENGTH) {
                if let Ok(length) = content_length.to_str() {
                    if let Ok(length) = length.parse::<u64>() {
                        return Ok(Some(length))
                    }
                }
            }

            Ok(None)
        },
        Err(err) => {
            Err(DownloadError::HttpRequestError(err))
        }
    }
}