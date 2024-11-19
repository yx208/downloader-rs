use reqwest::{Client, Request, header::CONTENT_LENGTH};
use url::Url;

pub fn clone_request(request: &Request) -> Request {
    let mut req = Request::new(request.method().clone(), request.url().clone());
    *req.headers_mut() = request.headers().clone();
    *req.version_mut() = request.version();
    *req.timeout_mut() = request.timeout().map(Clone::clone);

    req
}

pub async fn get_file_length(url: Url) -> Option<u64> {
    let request = Request::new(reqwest::Method::HEAD, url);
    let client = Client::new();

    match client.execute(request).await {
        Ok(response) => {
            if let Some(content_length) = response.headers().get(CONTENT_LENGTH) {
                if let Ok(length) = content_length.to_str() {
                    if let Ok(length) = length.parse::<u64>() {
                        return Some(length)
                    }
                }
            }

            None
        },
        Err(_) => None
    }
}