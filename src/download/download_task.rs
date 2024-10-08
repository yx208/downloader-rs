//!
//! # 下载任务模块
//!

use std::sync::Arc;
use std::time::Duration;
use std::io::SeekFrom;

use anyhow::{Result, Context};
use futures_util::stream::StreamExt;
use log::{info, error};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::fs::{OpenOptions, File};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Deserialize, Serialize, Clone)]
pub struct DownloadTaskState {
    pub id: Uuid,
    pub url: String,
    pub file_path: String,
    pub file_size: u64,
    pub downloaded: u64,
    pub status: TaskStatus,
    pub chunk_size: u64,
    pub retry_times: usize
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum TaskStatus {
    Pending,
    Downloading,
    Paused,
    Completed,
    Failed,
    Canceled
}

pub struct DownloadTask {
    // 多线程共享, 每次更改状态应先 lock, 只读使用 clone
    pub state: Arc<RwLock<DownloadTaskState>>,
    // Client 内部拥有一个连接池且默认拥有一个 Arc 包裹，所以应尽量使用 clone 复用
    pub client: Client,
}

impl DownloadTask {
    pub fn new(url: String, file_path: String, chunk_size: u64, retry_times: usize) -> Self {
        let client = Client::new();
        let state = DownloadTaskState {
            id: Uuid::new_v4(),
            chunk_size,
            retry_times,
            file_path,
            file_size: 0,
            downloaded: 0,
            url: url.clone(),
            status: TaskStatus::Pending
        };

        DownloadTask {
            client,
            state: Arc::new(RwLock::new(state)),
        }
    }

    pub async fn pause(&self) {
        let mut state = self.state.write().await;
        if state.status == TaskStatus::Downloading {
            state.status = TaskStatus::Paused;
            info!("Task pause requested: {}", state.file_path);
        }
    }

    pub async fn cancel(&self) {
        let mut state = self.state.write().await;
        state.status = TaskStatus::Canceled;
        info!("Task cancellation requested: {}", state.file_path);
    }
}

/// 下载任务
pub async fn download_task(state: Arc<RwLock<DownloadTaskState>>, client: Client) -> Result<TaskStatus> {
    // Initialize file_size if it's zero
    {
        let mut state_guard = state.write().await;
        if state_guard.file_size == 0 {
            state_guard.file_size = get_content_length(&client, &state_guard.url)
                .await
                .with_context(|| format!("Failed to get content length: {}", &state_guard.url))?;
        }
        state_guard.status = TaskStatus::Downloading;
    }

    let (url, file_path, file_size, chunk_size, retry_times) = {
        let state_guard = state.read().await;
        (
            state_guard.url.clone(),
            state_guard.file_path.clone(),
            state_guard.file_size,
            state_guard.chunk_size,
            state_guard.retry_times
        )
    };

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .open(&file_path)
        .await
        .with_context(|| format!("File chunk write failed: {}", &file_path))?;

    let mut start = {
        let state_guard = state.read().await;
        state_guard.downloaded
    };

    while start < file_size {
        {
            let state_guard = state.read().await;
            match state_guard.status {
                TaskStatus::Paused => {
                    info!("Download paused: {}", state_guard.file_path);
                    return Ok(TaskStatus::Paused);
                }
                TaskStatus::Canceled => {
                    info!("Download canceled: {}", state_guard.file_path);
                    return Ok(TaskStatus::Canceled);
                }
                TaskStatus::Failed => {
                    error!("Download failed: {}", state_guard.file_path);
                    return Err(anyhow::anyhow!("Task failed"));
                }
                _ => {}
            }
        }

        let end = ((start + chunk_size).min(file_size)) - 1;

        let result = download_chunk_with_retry(
            &client,
            &url,
            start,
            end,
            &mut file,
            retry_times,
            state.clone(),
        ).await;

        match result {
            Ok(bytes_downloaded) => {
                let mut state_guard = state.write().await;
                state_guard.downloaded += bytes_downloaded;
                start = state_guard.downloaded;
            }
            Err(err) => {
                error!("Chunk download failed: {}", err);
                let mut state_guard = state.write().await;
                state_guard.status = TaskStatus::Failed;
                return Err(err);
            }
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    Ok(TaskStatus::Completed)
}

/// 为下载的 chunk 添加重试
async fn download_chunk_with_retry(
    client: &Client,
    url: &str,
    start: u64,
    end: u64,
    file: &mut File,
    retry_times: usize,
    state: Arc<RwLock<DownloadTaskState>>,
) -> Result<u64> {
    let mut attempts = 0;
    loop {
        match download_chunk(client, url, start, end, file, state.clone()).await {
            Ok(bytes_downloaded) => return Ok(bytes_downloaded),
            Err(e) => {
                attempts += 1;
                if attempts >= retry_times {
                    return Err(e);
                } else {
                    error!("Download failed, retrying ({}/{}): {}", attempts, retry_times, e);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
}

/// 下载单个 chunk
async fn download_chunk(
    client: &Client,
    url: &str,
    start: u64,
    end: u64,
    file: &mut File,
    state: Arc<RwLock<DownloadTaskState>>
) -> Result<u64> {
    let range_header = format!("bytes={}-{}", start, end);
    let resp = client
        .get(url)
        .header(reqwest::header::RANGE, range_header)
        .send()
        .await
        .with_context(|| format!("Failed to send request: {}", url))?;

    // 请求成功，以流的形式读取
    if resp.status() == reqwest::StatusCode::PARTIAL_CONTENT || resp.status().is_success() {
        let mut stream = resp.bytes_stream();
        let mut total_bytes: u64 = 0;

        file.seek(SeekFrom::Start(start)).await?;

        // Buffer to hold data before writing to disk
        let mut buffer = Vec::new();
        let buffer_size = 1024 * 1024 * 5; // 5MB

        while let Some(item) = stream.next().await {
            let chunk = item?;
            buffer.extend_from_slice(&chunk);

            // If buffer is full, write to disk
            if buffer.len() >= buffer_size {
                file.write_all(&buffer).await?;
                total_bytes += buffer.len() as u64;
                buffer.clear();
            }

            // Check task status
            let state_guard = state.read().await;
            match state_guard.status {
                TaskStatus::Paused | TaskStatus::Canceled => {
                    // Write remaining buffer to disk
                    if !buffer.is_empty() {
                        file.write_all(&buffer).await?;
                        total_bytes += buffer.len() as u64;
                        buffer.clear();
                    }

                    info!("Download interrupted: {}", state_guard.file_path);
                    return Ok(total_bytes);
                }
                _ => {}
            };
        }

        if !buffer.is_empty() {
            file.write_all(&buffer).await?;
            total_bytes += buffer.len() as u64;
            buffer.clear();
        }

        Ok(total_bytes)
    } else {
        Err(anyhow::anyhow!("Download failed: HTTP {}", resp.status()))
    }
}

async fn get_content_length(client: &Client, url: &str) -> Result<u64> {
    let resp = client
        .head(url)
        .send()
        .await
        .with_context(|| format!("Failed to send HEAD request: {}", url))?;

    if resp.status().is_success() {
        if let Some(len) = resp.headers().get(reqwest::header::CONTENT_LENGTH) {
            let len = len
                .to_str()?
                .parse::<u64>()
                .with_context(|| "Failed to parse Content-Length")?;

            Ok(len)
        } else {
            Err(anyhow::anyhow!("Content-Length not found"))
        }
    } else {
        Err(anyhow::anyhow!("Failed to get content length: HTTP {}", resp.status()))
    }
}
