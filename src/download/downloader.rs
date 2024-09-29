//!
//! # 下载器
//! 管理多个 DownloadTask 实例，支持添加、暂停、恢复、删除任务，以及保存/加载状态
//!

use std::sync::Arc;
use std::path::Path;

use anyhow::Result;
use dashmap::DashMap;
use log::{info, error};
use tokio::sync::{Mutex, Semaphore};
use uuid::Uuid;
use crate::download::download_task::{DownloadTask, DownloadTaskState, TaskStatus};
use crate::download::persistence::PersistenceState;

pub struct Downloader {
    // 跨线程读写包裹
    tasks: Arc<DashMap<Uuid, Arc<Mutex<DownloadTask>>>>,
    // 线程限制
    semaphore: Arc<Semaphore>,
    // 状态文件路径
    state_file: String
}

impl Downloader {
    pub fn new(max_concurrent_downloads: usize, state_file: String) -> Self {
        Downloader {
            state_file,
            tasks: Arc::new(DashMap::new()),
            semaphore: Arc::new(Semaphore::new(max_concurrent_downloads))
        }
    }

    /// 添加新下载任务
    pub async fn add_task(&self, url: String, file_path: String, chunk_size: u64, retry_times: usize) -> Result<Uuid> {
        let mut task = DownloadTask::new(url.clone(), file_path, chunk_size, retry_times, self.semaphore.clone()).await?;
        let task_id;
        {
            let state_guard = task.state.lock().await;
            task_id = state_guard.id.clone();
        }
        task.start().await;

        let task = Arc::new(Mutex::new(task));
        self.tasks.insert(task_id, task);
        self.save_state().await?;

        Ok(task_id.clone())
    }

    /// 暂停任务
    pub async fn pause_task(&self, id: Uuid) {
        if let Some(task) = self.tasks.get(&id) {
            task.value().lock().await.pause().await;
        }
        self.save_state().await.unwrap_or_else(|e| error!("Failed to save state: {}", e));
    }

    /// 恢复任务
    pub async fn resume_task(&self, id: Uuid) {
        if let Some(task) = self.tasks.get(&id) {
            let mut task = task.value().lock().await;
            task.resume().await;
        }
        self.save_state().await.unwrap_or_else(|e| error!("Failed to save state: {}", e));
    }

    pub async fn cancel_task(&self, id: Uuid) {
        if let Some(task) = self.tasks.get(&id) {
            task.value().lock().await.cancel().await;
        }
        self.save_state()
            .await
            .unwrap_or_else(|e| error!("Failed to save state: {}", e));
    }

    /// 删除任务
    pub async fn delete_task(&self, id: &Uuid) {
        if let Some((_, task)) = self.tasks.remove(id) {
            let task_guard = task.lock().await;
            task_guard.cancel().await;
            let state_guard = task_guard.state.lock().await;
            if Path::new(&state_guard.file_path).exists() {
                tokio::fs::remove_file(&state_guard.file_path)
                    .await
                    .unwrap_or_else(|e| error!("Failed to delete file: {}", e));
            }
            info!("Task deleted: {}", state_guard.file_path);
        }
        self.save_state().await.unwrap_or_else(|e| error!("Failed to save state: {}", e));
    }

    /// 获取所有任务的状态
    pub async fn get_tasks(&self, ) -> Vec<DownloadTaskState> {
        let mut task_states= Vec::new();
        for entry in self.tasks.iter() {
            let task = entry.value();
            let state_guard = task.lock().await.state.lock().await.clone();
            task_states.push(state_guard);
        }

        task_states
    }

    /// 保存下载状态
    pub async fn save_state(&self) -> Result<()> {
        let tasks = self.get_tasks().await;
        let persistent_state = PersistenceState { tasks };
        persistent_state.save_to_file(&self.state_file)?;

        Ok(())
    }

    /// 加载状态
    pub async fn load_state(&self) -> Result<()> {
        let persistent_state = PersistenceState::load_from_file(&self.state_file)?;
        for task_state in persistent_state.tasks {
            // 创建任务实例
            let mut task = DownloadTask {
                semaphore: self.semaphore.clone(),
                state: Arc::new(Mutex::new(task_state.clone())),
                client: reqwest::Client::new(),
                handle: None
            };

            if task_state.status == TaskStatus::Downloading || task_state.status == TaskStatus::Pending {
                task.start().await;
            }

            let task_id = task.state.lock().await.id.clone();
            let task = Arc::new(Mutex::new(task));
            self.tasks.insert(task_id, task);
        }

        Ok(())
    }

    pub async fn remove_completed_tasks(&self) {
        let mut tasks_to_remove = Vec::new();

        for entry in self.tasks.iter() {
            let task_id = *entry.key();
            let task = entry.value();
            let state = task.lock().await.state.lock().await.clone();
            if state.status == TaskStatus::Completed {
                tasks_to_remove.push(task_id);
            }
        }

        for task_id in tasks_to_remove {
            self.tasks.remove(&task_id);
        }
    }
}
