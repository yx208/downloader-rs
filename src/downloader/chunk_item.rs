use tokio_util::sync::CancellationToken;
use anyhow::Result;
use reqwest::{Client, Error, Request, Response};
use crate::downloader::error::{DownloadEndCause, DownloadError};

pub struct ChunkItem {
    client: Client,
    cancel_token: CancellationToken
}

impl ChunkItem {
    pub fn new(client: Client, cancel_token: CancellationToken) -> Self {
        Self {
            client,
            cancel_token
        }
    }

    pub async fn download(&self, request: Request) -> Result<()> {
        let future = self.execute_download(request);

        tokio::select! {
            res = future => {
                res?;

                Ok(())
            }
            _ = self.cancel_token.cancelled() => {
                println!("Cancelled");
                Ok(())
            }
        }
    }

    async fn execute_download(&self, request: Request) -> Result<(), DownloadError> {
        'r: loop {
            let response = match self.client.execute(request).await {
                Ok(_) => {

                }
                Err(err) => {
                    return Err(DownloadError::HttpRequestError(err));
                }
            };

            return Ok(())
        }
    }
}

mod tests {
    use url::Url;
    use super::*;

    #[tokio::test]
    async fn should_be_run() {
        let token = CancellationToken::new();
        let client = Client::new();
        let item = ChunkItem::new(client, token.clone());

        let file_url = "";
        let request = Request::new(reqwest::Method::GET, Url::parse().unwrap());

        item.download().await.unwrap();
    }
}
