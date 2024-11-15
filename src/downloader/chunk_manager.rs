use reqwest::Request;

pub struct ChunkManger {}

impl ChunkManger {
    pub fn new() -> Self {
        Self {}
    }

    pub fn clone_request(request: &Request) -> Request {
        let mut req = Request::new(request.method().clone(), request.url().clone());
        *req.headers_mut() = request.headers().clone();
        *req.version_mut() = request.version();
        *req.timeout_mut() = request.timeout().map(Clone::clone);

        req
    }
}
