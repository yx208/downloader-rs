//!
//! # 下载器
//! 管理多个 DownloadTask 实例，支持添加、暂停、恢复、删除任务，以及保存/加载状态
//!

use std::sync::Arc;
use std::path::{Path, PathBuf};

use anyhow::Result;
use dashmap::DashMap;
use log::{info, error};
use tokio::sync::RwLock;
use uuid::Uuid;
use async_channel::Sender;

use crate::download::download_task::{DownloadTask, DownloadTaskState, TaskStatus};

pub type DownloaderTasks = Arc<DashMap<Uuid, Arc<RwLock<DownloadTask>>>>;

pub struct Downloader {
    tasks: DownloaderTasks,
    pending_sender: Sender<Uuid>,
}

impl Downloader {
    pub fn new(tasks: DownloaderTasks, pending_sender: Sender<Uuid>) -> Self {
        Self {
            tasks,
            pending_sender
        }
    }

    /// 添加新下载任务
    pub async fn add_task(&self, url: String, mut file_path: String, chunk_size: u64, retry_times: usize) -> Result<Uuid> {
        // Handle existing file names
        file_path = self.handle_existing_file(&file_path).await;

        let task = DownloadTask::new(url.clone(), file_path, chunk_size, retry_times);
        let task_id = task.state.read().await.id;
        let task = Arc::new(RwLock::new(task));

        self.tasks.insert(task_id, task.clone());
        self.pending_sender.send(task_id).await?;

        Ok(task_id)
    }

    /// Handle same file
    async fn handle_existing_file(&self, file_path: &str) -> String {
        let (parent_dir, file_stem, extension) = parse_filename(file_path);
        let tasks: Vec<PathBuf> = self.get_tasks().await
            .iter()
            .map(|t| PathBuf::from(&t.file_path))
            .collect();

        let mut path = PathBuf::from(file_path);
        let mut counter = 1;

        while path.exists() || tasks.contains(&path) {
            let new_filename = if extension.is_empty() {
                format!("{}_{}", file_stem, counter)
            } else {
                format!("{}_{}.{}", file_stem, counter, extension)
            };

            path = parent_dir.join(new_filename);
            counter += 1;
        }

        path.to_str().unwrap().to_string()
    }

    /// Get all task state
    pub async fn get_tasks(&self) -> Vec<DownloadTaskState> {
        let mut task_states = Vec::new();
        for entry in self.tasks.iter() {
            let task_guard = entry.value().read().await;
            let state_guard = task_guard.state.read().await;
            task_states.push(state_guard.clone());
        }

        task_states
    }

    /// Pause
    pub async fn pause_task(&self, id: Uuid) {
        if let Some(task) = self.tasks.get(&id) {
            task.value().read().await.pause().await;
        }
    }

    /// Resume
    pub async fn resume_task(&self, id: Uuid) {
        if let Some(task) = self.tasks.get(&id) {
            let task_guard = task.value().read().await;
            let mut state_guard = task_guard.state.write().await;
            if state_guard.status == TaskStatus::Paused {
                state_guard.status = TaskStatus::Pending;
                // Re-add to pending queue
                self.pending_sender.send(id).await.unwrap();
                info!("Task resumed: {}", state_guard.file_path);
            }
        }
    }

    /// Cancel
    pub async fn cancel_task(&self, id: Uuid) {
        if let Some(task) = self.tasks.get(&id) {
            task.value().read().await.cancel().await;
        }
    }

    /// Delete, remove from tasks
    pub async fn delete_task(&self, id: Uuid) {
        if let Some(task) = self.tasks.remove(&id) {
            let task = task.1;
            let task = task.read().await;
            // Ensure the download is stopped
            task.cancel().await;

            let state = task.state.read().await.clone();
            if Path::new(&state.file_path).exists() {
                tokio::fs::remove_file(&state.file_path)
                    .await
                    .unwrap_or_else(|e| error!("Failed to delete file: {}", e));
            }
            info!("Task deleted: {}", state.file_path);
        }
    }

    /// Remove completed tasks, from tasks
    pub async fn remove_completed_tasks(&self) {
        let mut tasks_to_remove = Vec::new();

        for entry in self.tasks.iter() {
            let task_id = *entry.key();
            let task = entry.value();
            let state = task.read().await.state.read().await.clone();
            if state.status == TaskStatus::Completed {
                tasks_to_remove.push(task_id);
            }
        }

        for task_id in tasks_to_remove {
            self.tasks.remove(&task_id);
        }
    }
}

fn parse_filename(file_path: &str) -> (PathBuf, String, String) {
    let original_path = PathBuf::from(file_path);
    let parent_dir = original_path.parent().unwrap().to_owned();
    let file_stem = original_path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let extension = original_path.extension()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_default();

    (parent_dir, file_stem, extension)
}
