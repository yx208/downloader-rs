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
use async_channel::{Sender, Receiver};

use crate::download::download_task::{DownloadTask, DownloadTaskState, TaskStatus};
use crate::download::download_task::TaskStatus::Pending;
use crate::download::persistence::PersistenceState;

pub struct Downloader {
    // 跨线程读写包裹
    tasks: Arc<DashMap<Uuid, Arc<Mutex<DownloadTask>>>>,
    // 线程限制
    semaphore: Arc<Semaphore>,
    // 状态文件路径
    state_file: String,
    pending_sender: Sender<Uuid>,
    pending_receiver: Receiver<Uuid>,
}

impl Downloader {
    pub fn new(max_concurrent_downloads: usize, state_file: String) -> Arc<Self> {
        let (pending_sender, pending_receiver) = async_channel::unbounded();
        let downloader = Arc::new(Downloader {
            state_file,
            tasks: Arc::new(DashMap::new()),
            semaphore: Arc::new(Semaphore::new(max_concurrent_downloads)),
            pending_sender,
            pending_receiver
        });

        downloader.clone().start_scheduler();
        downloader
    }

    /// 添加新下载任务
    pub async fn add_task(&self, url: String, mut file_path: String, chunk_size: u64, retry_times: usize) -> Result<Uuid> {
        file_path = self.handle_existing_file(&file_path).await;

        let mut task = DownloadTask::new(url.clone(), file_path, chunk_size, retry_times, self.semaphore.clone());
        let task_id = task.state.lock().await.id.clone();
        let task = Arc::new(Mutex::new(task));

        self.tasks.insert(task_id.clone(), task);
        self.pending_sender.send(task_id.clone()).await?;
        self.save_state().await?;

        Ok(task_id)
    }

    /// 处理文件夹中文件重名问题
    async fn handle_existing_file(&self, file_path: &str) -> String {
        let mut new_file_path = file_path.to_string();
        let mut counter = 1;
        while Path::new(&new_file_path).exists() {
            let extension = Path::new(file_path)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let file_stem = Path::new(file_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            if extension.is_empty() {
                new_file_path = format!("{}_{}", file_stem, counter);
            } else {
                new_file_path = format!("{}_{}.{}", file_stem, counter, extension);
            }

            counter += 1;
        }

        new_file_path
    }

    fn start_scheduler(self: Arc<Self>) {
        let downloader = self.clone();
        tokio::spawn(async move {
            loop {
                let task_id = match downloader.pending_receiver.recv().await {
                    Ok(task_id) => task_id,
                    Err(_) => break
                };

                let permit = downloader.semaphore.clone().acquire_owned().await.unwrap();

                let task = {
                    if let Some(task) = downloader.tasks.get(&task_id) {
                        task.clone()
                    } else {
                        drop(permit);
                        continue;
                    }
                };

                {
                    let mut task_guard = task.lock().await;
                    let mut state_guard = task_guard.state.lock().await;
                    state_guard.status = TaskStatus::Downloading;
                }

                let downloader_clone = downloader.clone();
                let task_clone = task.clone();
            }
        });
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
    pub async fn delete_task(&self, id: Uuid) {
        if let Some((_, task)) = self.tasks.remove(&id) {
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
