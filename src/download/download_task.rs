//! # 下载任务模块

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::io::SeekFrom;

use anyhow::{Result, Context};
use futures_util::stream::StreamExt;
use log::{info, error};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::{Semaphore, Mutex};
use tokio::task::JoinHandle;
use crate::utils::get_content_length;

#[derive(Deserialize, Serialize, Clone)]
pub struct DownloadTaskState {
    pub url: String,
    pub file_path: String,
    pub file_size: u64,
    pub downloaded: u64,
    pub status: TaskStatus,
    pub chunk_size: u64,
    pub retry_times: usize
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum TaskStatus {
    Pending,
    Downloading,
    Paused,
    Completed,
    Failed
}

pub struct DownloadTask {
    // 多线程共享, 每次更改状态应先 lock, 只读使用 clone
    pub state: Arc<Mutex<DownloadTaskState>>,
    // Client 内部拥有一个连接池且默认拥有一个 Arc 包裹，所以应尽量使用 clone 复用
    pub client: Client,
    pub handle: Option<JoinHandle<()>>
}

impl DownloadTask {
    pub async fn new(url: String, file_path: String, chunk_size: u64, retry_times: usize) -> Result<Self> {
        let client = Client::new();
        let file_size = get_content_length(&client, &url)
            .await
            .with_context(|| format!("Get file size failed: {}", &url))?;

        if !Path::new(&file_path).exists() {
            let file = fs::File::create(&file_path)?;
            file.set_len(file_size)?;
        }

        let state = DownloadTaskState {
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
            state: Arc::new(Mutex::new(state)),
            handle: None
        })
    }

    pub async fn start(&mut self, semaphore: Arc<Semaphore>) {
        let state = self.state.clone();
        let client = self.client.clone();

        // 'acquire_owned' 才能跨线程, 申请线程许可，如果线程使用满了则等待
        let permit = semaphore.acquire_owned().await.unwrap();

        // 确保在锁外创建异步任务
        self.handle = Some(tokio::spawn(async move {
            {
                let mut state_guard = state.lock().await;
                if state_guard.status == TaskStatus::Completed {
                    info!("Task completed: {}", state_guard.file_path);
                    return;
                }
                state_guard.status = TaskStatus::Downloading;
            }

            // 根据下载情况，把任务置为不同状态
            if let Err(e) = download_task(state.clone(), client).await {
                error!("下载失败：{}", e);
                let mut state_guard = state.lock().await;
                state_guard.status = TaskStatus::Failed;
            } else {
                let mut state_guard = state.lock().await;
                state_guard.status = TaskStatus::Completed;
                info!("下载完成：{}", state_guard.file_path);
            }

            // 释放线程数量
            drop(permit);
        }));
    }

    pub async fn pause(&self) {
        let mut state = self.state.lock().await;
        if state.status == TaskStatus::Downloading {
            state.status = TaskStatus::Paused;
            info!("Task has been paused：{}", state.file_path);
        }
    }

    pub async fn resume(&mut self, semaphore: Arc<Semaphore>) {
        let state = self.state.clone();
        let client = self.client.clone();

        // 如果没有异步任务或者任务已经完成, 重新添加任务
        if self.handle.is_none() || self.handle.as_ref().unwrap().is_finished() {
            self.handle = Some(tokio::spawn(async move {
                // 请求所有权许可
                let permit = semaphore.acquire_owned().await.unwrap();

                let mut state_guard = state.lock().await;
                state_guard.status = TaskStatus::Downloading;
                drop(state_guard);

                if let Err(e) = download_task(state.clone(), client).await {
                    error!("Download failed: {}", e);
                    let mut state_guard = state.lock().await;
                    state_guard.status = TaskStatus::Failed;
                } else {
                    let mut state_guard = state.lock().await;
                    state_guard.status = TaskStatus::Completed;
                    info!("Download completed：{}", state_guard.file_path);
                }

                drop(permit);
            }));
        }
    }

    pub fn is_finished(&self) -> bool {
        if let Some(handle) = &self.handle {
            handle.is_finished()
        } else {
            false
        }
    }
}

async fn download_task(state: Arc<Mutex<DownloadTaskState>>, client: Client) -> Result<()> {
    let state_guard = state.lock().await;
    let url = state_guard.url.clone();
    let file_path = state_guard.file_path.clone();
    let file_size = state_guard.file_size;
    let mut downloaded = state_guard.downloaded;
    let chunk_size = state_guard.chunk_size;
    let retry_times = state_guard.retry_times;
    drop(state_guard);

    let mut file = OpenOptions::new()
        .write(true)
        .open(&file_path)
        .await
        .with_context(|| format!("File chunk write failed: {}", &file_path))?;

    while downloaded < file_size {
        // 检查是否暂停
        {
            let state_guard = state.lock().await;
            if state_guard.status == TaskStatus::Paused {
                info!("The download task has been paused: {}", state_guard.file_path);
                break;
            }
        }

        let start = downloaded;
        let end = ((start + chunk_size).min(file_size)) - 1;

        let result = download_chunk_with_retry(
            &client,
            &url,
            start,
            end,
            &mut file,
            retry_times
        ).await;

        match result {
            Ok(bytes_downloaded) => {
                downloaded += bytes_downloaded;

                let mut state_guard = state.lock().await;
                state_guard.downloaded = downloaded;
                drop(state_guard);
            },
            Err(e) => {
                error!("Download chunk failed: {}", e);
                return Err(e);
            }
        }
    }

    Ok(())
}

/// 为下载的 chunk 添加重试
async fn download_chunk_with_retry(
    client: &Client,
    url: &str,
    start: u64,
    end: u64,
    file: &mut tokio::fs::File,
    retry_times: usize
) -> Result<u64> {
    let mut attempts = 0;
    loop {
        match download_chunk(client, url, start, end, file).await {
            Ok(_) => {},
            Err(error) => {
                attempts += 1;
                if attempts > retry_times {
                    return Err(error);
                } else {
                    error!("Download failed, try again {}/{}:{}", attempts, retry_times, error);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
}

/// 下载单个 chunk
async fn download_chunk(client: &Client, url: &str, start: u64, end: u64, file: &mut tokio::fs::File) -> Result<u64> {
    let range_header = format!("bytes={}-{}", start, end);
    let resp = client
        .get(url)
        .header(reqwest::header::RANGE, range_header)
        .send()
        .await
        .with_context(|| format!("Send request failed: {}", url))?;

    if resp.status() == reqwest::StatusCode::PARTIAL_CONTENT || resp.status().is_success() {
        let mut stream = resp.bytes_stream();
        let mut total_bytes = 0;

        file.seek(SeekFrom::Start(start)).await?;

        while let Some(item) = stream.next().await {
            let chunk = item?;
            file.write_all(&chunk).await?;
            total_bytes += chunk.len() as u64;
        }

        Ok(total_bytes)
    } else {
        Err(anyhow::anyhow!("Download failed: HTTP {}", resp.status()))
    }
}

