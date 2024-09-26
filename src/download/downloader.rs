//!
//! # 下载器
//! 管理多个 DownloadTask 实例，支持添加、暂停、恢复、删除任务，以及保存/加载状态
//!

use std::sync::Arc;
use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use log::{info, error};
use tokio::sync::Mutex;
use tokio::sync::Semaphore;

use crate::download::download_task::{DownloadTask, DownloadTaskState, TaskStatus};
use crate::download::persistence::PersistenceState;

// 简化类型定义抽出来的
// DownloadTask 需要在多线程共享修改，需要包裹 Arc/Mutex
type DownloaderTask = HashMap<String, Arc<Mutex<DownloadTask>>>;

pub struct Downloader {
    // 跨线程读写包裹
    tasks: Arc<Mutex<DownloaderTask>>,
    // 线程限制
    semaphore: Arc<Semaphore>,
    // 状态文件路径
    state_file: String
}

impl Downloader {
    pub fn new(max_concurrent_downloads: usize, state_file: String) -> Self {
        Downloader {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            semaphore: Arc::new(Semaphore::new(max_concurrent_downloads)),
            state_file
        }
    }

    /// 添加新下载任务
    pub async fn add_task(&self, url: String, file_path: String, chunk_size: u64, retry_times: usize) -> Result<()> {
        let mut task = DownloadTask::new(url.clone(), file_path, chunk_size, retry_times).await?;
        task.start(self.semaphore.clone()).await;

        let task = Arc::new(Mutex::new(task));

        self.tasks.lock().await.insert(url.clone(), task.clone());

        self.save_state()?;

        Ok(())
    }

    /// 暂停任务
    pub async fn pause_task(&self, url: &str) {
        if let Some(task) = self.tasks.lock().await.get(url) {
            task.lock().await.pause().await;
            self.save_state().await.unwrap_or_else(|e| error!("Save state failed: {}", e));
        }
    }

    /// 恢复任务
    pub async fn resume_task(&self, url: &str) {
        if let Some(task) = self.tasks.lock().await.get(url) {
            task.lock().await.resume(self.semaphore.clone()).await;
            self.save_state().await.unwrap_or_else(|e| error!("Save state failed: {}", e));
        }
    }

    /// 删除任务
    pub async fn delete_task(&self, url: &str) {
        if let Some(task) = self.tasks.lock().await.remove(url) {
            let task_guard = task.lock().await;
            let task_state = task_guard.state.lock().await;
            if Path::new(&task_state.file_path).exists() {
                std::fs::remove_file(&task_state.file_path).unwrap_or_else(|e| error!("Delete file failed: {}", e));
            }
            self.save_state().await.unwrap_or_else(|e| error!("Save state failed: {}", e));
            info!("The task has been deleted: {}", url);
        }
    }

    /// 获取所有任务的状态
    pub async fn get_tasks(&self, ) -> Vec<DownloadTaskState> {
        let mut tasks= Vec::new();
        for task in self.tasks.lock().await.values() {
            let task_guard = task.lock().await;
            tasks.push(task_guard.state.lock().await.clone());
        }

        tasks
    }

    /// 保存下载状态
    pub async fn save_state(&self) -> Result<()> {
        let tasks = self.tasks.lock().await;
        let mut states = Vec::new();

        for task in tasks.values() {
            let task_clone = task.lock().await.state.lock().await.clone();
            states.push(task_clone);
        }

        let persistent_state = PersistenceState { tasks: states };
        persistent_state.save_to_file(&self.state_file)?;

        Ok(())
    }

    /// 加载状态
    pub async fn load_state(&self) -> Result<()> {
        let persistent_state = PersistenceState::load_from_file(&self.state_file)?;
        for task_state in persistent_state.tasks {
            // 创建任务实例
            let task = DownloadTask {
                state: Arc::new(Mutex::new(task_state.clone())),
                client: reqwest::Client::new(),
                handle: None
            };

            let task = Arc::new(Mutex::new(task));
            self.tasks.lock().await.insert(task_state.url.clone(), task.clone());

            // 恢复下载任务
            if task_state.status == TaskStatus::Downloading || task_state.status == TaskStatus::Pending {
                task.lock().await.start(self.semaphore.clone()).await;
            }
        }

        Ok(())
    }
}
