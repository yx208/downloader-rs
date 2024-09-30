//!
//! # 下载任务模块
//!

use std::path::Path;
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
use tokio::sync::{Semaphore, Mutex};
use tokio::task::JoinHandle;
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
    pub state: Arc<Mutex<DownloadTaskState>>,
    // Client 内部拥有一个连接池且默认拥有一个 Arc 包裹，所以应尽量使用 clone 复用
    pub client: Client,
    // tokio::spawn JoinHandle
    pub handle: Option<JoinHandle<()>>,
    // 信号量
    pub semaphore: Arc<Semaphore>,
}

impl DownloadTask {
    pub async fn new(
        url: String,
        file_path: String,
        chunk_size: u64,
        retry_times: usize,
        semaphore: Arc<Semaphore>,
    ) -> Result<Self> {
        let client = Client::new();
        let file_size = get_content_length(&client, &url)
            .await
            .with_context(|| format!("Get file size failed: {}", &url))?;

        if !Path::new(&file_path).exists() {
            let file = File::create(&file_path).await?;
            file.set_len(file_size).await?;
        }

        let state = DownloadTaskState {
            id: Uuid::new_v4(),
            file_size,
            chunk_size,
            retry_times,
            downloaded: 0,
            url: url.clone(),
            file_path: file_path.clone(),
            status: TaskStatus::Pending
        };

        Ok(DownloadTask {
            client,
            semaphore,
            handle: None,
            state: Arc::new(Mutex::new(state)),
        })
    }

    pub async fn start(&mut self) {
        {
            let state = self.state.clone();
            let mut state_guard = state.lock().await;
            // 非暂停或者失败状态，不能恢复下载
            if state_guard.status != TaskStatus::Pending {
                return;
            }
            state_guard.status = TaskStatus::Downloading;
        }

        self.run_task().await;
    }

    pub async fn pause(&self) {
        let mut state = self.state.lock().await;
        if state.status == TaskStatus::Downloading {
            state.status = TaskStatus::Paused;
            info!("Task has been paused: {}", state.file_path);
        }
    }

    async fn run_task(&mut self) {
        let handle_state = self.state.clone();
        let client = self.client.clone();
        let semaphore = self.semaphore.clone();
        let permit = semaphore.acquire_owned().await.unwrap();

        self.handle = Some(tokio::spawn(async move {
            let result = download_task(handle_state.clone(), client).await;
            let mut state_guard = handle_state.lock().await;

            match result {
                Ok(task_state) => state_guard.status = task_state,
                Err(_) => state_guard.status = TaskStatus::Failed
            };

            drop(permit);
        }));
    }

    pub async fn resume(&mut self) {
        {
            let state = self.state.clone();
            let mut state_guard = state.lock().await;
            // 非暂停或者失败状态，不能恢复下载
            if state_guard.status != TaskStatus::Paused || state_guard.status != TaskStatus::Failed {
                return;
            }
            state_guard.status = TaskStatus::Downloading;
        }

        self.run_task().await;
    }

    pub async fn cancel(&self) {
        let mut state = self.state.lock().await;
        state.status = TaskStatus::Canceled;
        info!("Task cancellation requested: {}", state.file_path);
    }

    pub fn is_finished(&self) -> bool {
        if let Some(handle) = &self.handle {
            handle.is_finished()
        } else {
            false
        }
    }
}

/// 下载任务
async fn download_task(state: Arc<Mutex<DownloadTaskState>>, client: Client) -> Result<TaskStatus> {
    let url;
    let file_path;
    let file_size;
    let chunk_size;
    let retry_times;

    {
        let state_guard = state.lock().await;
        url = state_guard.url.clone();
        file_path = state_guard.file_path.clone();
        file_size = state_guard.file_size;
        chunk_size = state_guard.chunk_size;
        retry_times = state_guard.retry_times;
    }

    let mut file = OpenOptions::new()
        .write(true)
        .open(&file_path)
        .await
        .with_context(|| format!("File chunk write failed: {}", &file_path))?;

    loop {
        let start = {
            let state_guard = state.lock().await;

            match state_guard.status {
                TaskStatus::Paused => {
                    info!("Download has been paused: {}", &file_path);
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
                _ => ()
            };

            if state_guard.downloaded >= file_size {
                info!("Download has been completed: {}", &file_path);
                return Ok(TaskStatus::Completed);
            }

            state_guard.downloaded
        };

        let end = ((start + chunk_size).min(file_size)) - 1;

        let result = download_chunk_with_retry(
            &client, &url,
            start, end,
            &mut file,
            retry_times,
            state.clone()
        ).await;

        match result {
            Ok(bytes_downloaded) => {
                let mut state_guard = state.lock().await;
                state_guard.downloaded += bytes_downloaded;
            },
            Err(e) => {
                error!("Download chunk failed: {}", e);
                return Err(e);
            }
        }
    }
}

/// 为下载的 chunk 添加重试
async fn download_chunk_with_retry(
    client: &Client,
    url: &str,
    start: u64,
    end: u64,
    file: &mut File,
    retry_times: usize,
    state: Arc<Mutex<DownloadTaskState>>,
) -> Result<u64> {
    let mut attempts = 0;
    loop {
        match download_chunk(client, url, start, end, file, state.clone()).await {
            Ok(bytes_downloaded) => return Ok(bytes_downloaded),
            Err(error) => {
                attempts += 1;
                if attempts > retry_times {
                    return Err(error);
                } else {
                    error!("Download failed, retrying ({}/{}): {}", attempts, retry_times, error);
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
    state: Arc<Mutex<DownloadTaskState>>
) -> Result<u64> {
    let range_header = format!("bytes={}-{}", start, end);
    let resp = client
        .get(url)
        .header(reqwest::header::RANGE, range_header)
        .send()
        .await
        .with_context(|| format!("Failed to send request: {}", url))?;

    if resp.status() == reqwest::StatusCode::PARTIAL_CONTENT || resp.status().is_success() {
        let mut stream = resp.bytes_stream();
        let mut total_bytes = 0;

        file.seek(SeekFrom::Start(start)).await?;

        while let Some(item) = stream.next().await {
            let chunk = item?;
            file.write_all(&chunk).await?;
            total_bytes += chunk.len() as u64;

            let state_guard = state.lock().await;
            match state_guard.status {
                TaskStatus::Paused | TaskStatus::Canceled => {
                    info!("Download interrupted: {}", state_guard.file_path);
                    return Ok(total_bytes);
                }
                _ => ()
            };
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::fs;

    #[tokio::test]
    async fn test_download_task() -> Result<()> {
        let mut download_dir = dirs::download_dir().unwrap();
        download_dir.push("demo.mp4");

        let url = "https://oss.xgy.tv/xgy/design/test/cool.mp4";
        let file_path = download_dir.to_str().unwrap();
        let chunk_size = 1024 * 1024 * 5;
        let retry_times = 3;

        // 删除测试文件（如果存在）
        if Path::new(file_path).exists() {
            fs::remove_file(file_path).await?;
        }

        let semaphore = Arc::new(Semaphore::new(1));
        let mut task = DownloadTask::new(
            url.to_string(),
            file_path.to_string(),
            chunk_size,
            retry_times,
            semaphore
        )
            .await?;

        task.start().await;

        // 等待任务完成
        while !task.is_finished() {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        let state = task.state.lock().await;
        assert_eq!(state.status, TaskStatus::Completed);

        // 检查文件是否存在
        assert!(Path::new(file_path).exists());

        // 清理测试文件
        fs::remove_file(file_path).await?;

        Ok(())
    }
}

