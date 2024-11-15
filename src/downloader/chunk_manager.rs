use reqwest::Request;
use crate::downloader::chunk_iterator::ChunkIterator;

pub struct ChunkManger {
    chunk_iter: ChunkIterator
}

impl ChunkManger {
    pub fn new(chunk_iter: ChunkIterator) -> Self {
        Self { chunk_iter }
    }

    pub fn clone_request(request: &Request) -> Request {
        let mut req = Request::new(request.method().clone(), request.url().clone());
        *req.headers_mut() = request.headers().clone();
        *req.version_mut() = request.version();
        *req.timeout_mut() = request.timeout().map(Clone::clone);

        req
    }

    pub async fn start_download() {

    }
}

mod tests {
    use super::*;

    #[tokio::test]
    async fn should_be_run() {

    }
}
