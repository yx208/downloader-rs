//!
//! # 下载器
//! 管理多个 DownloadTask 实例，支持添加、暂停、恢复、删除任务，以及保存/加载状态
//!

use std::sync::Arc;
use std::path::{Path, PathBuf};

use anyhow::Result;
use dashmap::DashMap;
use log::{info, error};
use tokio::sync::{Mutex, Semaphore};
use uuid::Uuid;
use async_channel::{Sender, Receiver};

use crate::download::download_task::{download_task, DownloadTask, DownloadTaskState, TaskStatus};
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
    pub fn new_default() -> Arc<Downloader> {
        let mut download_dir = dirs::download_dir().unwrap();
        download_dir.push("state.log");

        Self::new(3usize, String::from(download_dir.to_str().unwrap()))
    }

    pub fn new(max_concurrent_downloads: usize, state_file: String) -> Arc<Self> {
        let (pending_sender, pending_receiver) = async_channel::unbounded();
        let downloader = Arc::new(Downloader {
            tasks: Arc::new(DashMap::new()),
            semaphore: Arc::new(Semaphore::new(max_concurrent_downloads)),
            state_file,
            pending_sender,
            pending_receiver
        });

        downloader.clone().start_scheduler();

        downloader
    }

    /// 添加新下载任务
    pub async fn add_task(
        &self,
        url: String,
        mut file_path: String,
        chunk_size: u64,
        retry_times: usize
    ) -> Result<()> {
        file_path = self.handle_existing_file(&file_path).await;

        let mut task = DownloadTask::new(url.clone(), file_path, chunk_size, retry_times, self.semaphore.clone());
        let task_id = task.state.lock().await.id.clone();
        let task = Arc::new(Mutex::new(task));

        self.tasks.insert(task_id.clone(), task.clone());
        self.pending_sender.send(task_id).await?;
        self.save_state().await?;

        Ok(())
    }

    /// 处理文件夹中文件重名问题
    async fn handle_existing_file(&self, file_path: &str) -> String {
        let original_path = PathBuf::from(file_path);
        let parent_dir = original_path.parent().unwrap();
        let file_stem = original_path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let extension = original_path.extension()
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut path = original_path.clone();
        let mut counter = 1;

        while path.exists() {
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

    fn start_scheduler(self: Arc<Self>) {
        let downloader = self.clone();
        // 建立一个线程，循环处理任务调度
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

                // Execute download
                tokio::spawn(async move {
                    let client = task_clone.lock().await.client.clone();
                    let result = download_task(task_clone.lock().await.state.clone(), client).await;

                    let task_guard = task_clone.lock().await;
                    let mut state_guard = task_guard.state.lock().await;

                    match result {
                        Ok(TaskStatus::Completed) => {
                            state_guard.status = TaskStatus::Completed;
                            info!("Download completed: {}", state_guard.file_path);
                        }
                        Ok(TaskStatus::Paused) => {
                            state_guard.status = TaskStatus::Paused;
                            info!("Task paused: {}", state_guard.file_path);
                            downloader_clone.pending_sender.send(task_id).await.unwrap();
                        }
                        Ok(TaskStatus::Canceled) => {
                            state_guard.status = TaskStatus::Canceled;
                            info!("Task canceled: {}", state_guard.file_path);
                        }
                        Err(e) => {
                            state_guard.status = TaskStatus::Failed;
                            error!("Download failed: {}", e);
                        }
                        _ => ()
                    }

                    drop(permit);
                });
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
    pub async fn get_tasks(&self) -> Vec<DownloadTaskState> {
        let mut task_states= Vec::new();
        for entry in self.tasks.iter() {
            println!("1");
            let task_guard = entry.value().lock().await;
            println!("2");
            let state_guard = task_guard.state.lock().await.clone();
            println!("3");
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
