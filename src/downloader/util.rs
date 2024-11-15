use reqwest::{Client, Request};
use url::Url;
use crate::downloader::error::DownloadError;

pub async fn get_file_length(url: Url) -> anyhow::Result<Option<usize>, DownloadError> {
    let request = Request::new(reqwest::Method::HEAD, url);
    let client = Client::new();

    match client.execute(request).await {
        Ok(response) => {
            if let Some(content_length) = response.headers().get(reqwest::header::CONTENT_LENGTH) {
                if let Ok(length) = content_length.to_str() {
                    if let Ok(length) = length.parse::<usize>() {
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