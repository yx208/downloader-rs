use std::io::SeekFrom;
use std::sync::{Arc};
use std::sync::atomic::{AtomicU64, Ordering};
use reqwest::{Client, Request, Response};
use anyhow::Result;
use futures_util::StreamExt;
use headers::HeaderMapExt;
use tokio::sync::watch;
use tokio::fs::File;
use tokio::sync::Mutex;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::select;
use crate::download::chunk_range::{ChunkInfo, ChunkRange};
use crate::download::error::{DownloadEndCause, DownloadError};
use crate::download::util::clone_request;

pub struct ChunkItem {
    client: Client,
    file: Arc<Mutex<File>>,
    retry_count: u8,
    downloaded: AtomicU64,
    pub chunk_info: ChunkInfo
}

impl ChunkItem {
    pub fn new(chunk_info: ChunkInfo, file: Arc<Mutex<File>>, client: Client, retry_count: u8) -> Self {
        Self {
            client,
            file,
            retry_count,
            chunk_info,
            downloaded: AtomicU64::new(0),
        }
    }

    /// 执行下载 chunk，并进行 n 次重试
    pub async fn download(&self, request: Request, mut action_receiver: watch::Receiver<DownloadEndCause>) -> Result<DownloadEndCause, DownloadError> {
        let mut bytes_buffer = Vec::with_capacity(self.chunk_info.range.len() as usize);
        let future = async {
            // 写入 range 头
            let mut range_request = clone_request(&request);
            let header_map = range_request.headers_mut();
            header_map.typed_insert(self.chunk_info.range.clone().to_range_header());

            // 读取 chunk 数据
            let response = self.fetch_chunk(&range_request).await?;
            let mut stream = response.bytes_stream();
            while let Some(bytes) = stream.next().await {
                let bytes = match bytes {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        return Err(DownloadError::HttpRequestFailed(err));
                    }
                };
                let len = bytes.len();
                bytes_buffer.extend(&bytes);
                self.downloaded.fetch_add(len as u64, Ordering::Relaxed);
            }

            Ok(())
        };

        select! {
            result = future => {
                result?;

                let mut file = self.file.lock().await;
                file.seek(SeekFrom::Start(self.chunk_info.range.start)).await?;
                file.write_all(bytes_buffer.as_ref()).await?;
                file.flush().await?;
                file.sync_all().await?;

                Ok(DownloadEndCause::Finished)
            }
            _ = action_receiver.changed() => {
                let action = *action_receiver.borrow_and_update();
                Ok(action)
            }
        }
    }

    async fn fetch_chunk(&self, request: &Request) -> Result<Response, DownloadError> {
        let mut retry_count = 0;
        'a: loop {
            let response = match self.client.execute(clone_request(request)).await {
                Ok(response) => {
                    retry_count = 0;
                    response
                },
                Err(err) => {
                    retry_count += 1;

                    if retry_count > self.retry_count {
                        return Err(DownloadError::HttpRequestFailed(err));
                    }

                    continue 'a;
                }
            };

            break Ok(response);
        }
    }
}

mod tests {
    use tokio::fs::OpenOptions;
    use super::*;
    use crate::download::chunk_item::{ChunkInfo, ChunkItem};
    use dirs;
    use url::Url;

    async fn create_file(filename: &str) -> File {
        let mut download_dir = dirs::download_dir().unwrap();
        download_dir.push(filename);
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(download_dir)
            .await
            .unwrap();

        file
    }

    async fn create_chunk_item(filename: &str) -> ChunkItem {
        let chunk_info = ChunkInfo { index: 0, range: ChunkRange::new(0, 1024 * 1024 * 4) };
        let file = create_file(filename).await;
        let file = Arc::new(Mutex::new(file));
        let client = Client::new();
        let chunk_item = ChunkItem::new(chunk_info, file, client, 3);

        chunk_item
    }

    #[tokio::test]
    async fn should_be_download() {
        let chunk_item = create_chunk_item("demo.jpg").await;
        let url = Url::parse("http://localhost:23333/image.jpg").unwrap();
        let request = Request::new(reqwest::Method::GET, url);

        let (tx, rx) = watch::channel(DownloadEndCause::Finished);
        chunk_item.download(request, rx.clone()).await.unwrap();
    }
}
