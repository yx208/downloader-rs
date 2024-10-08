use std::sync::Arc;
use tokio::sync::{Semaphore};
use async_channel::{Receiver, Sender};
use log::{info, error};
use uuid::Uuid;

use crate::download::download_task::{download_task, TaskStatus};
use crate::download::downloader::DownloaderTasks;

pub struct Scheduler {
    tasks: DownloaderTasks,
    semaphore: Arc<Semaphore>,
    pending_receiver: Receiver<Uuid>,
    // Needed to re-add paused tasks
    pending_sender: Sender<Uuid>
}

impl Scheduler {
    pub fn new(
        tasks: DownloaderTasks,
        semaphore: Arc<Semaphore>,
        pending_receiver: Receiver<Uuid>,
        pending_sender: Sender<Uuid>
    ) -> Self {
        Self {
            tasks,
            semaphore,
            pending_receiver,
            pending_sender,
        }
    }

    /// Start scheduler
    pub async fn run(&self) {
        loop {
            // Wait for a task to become available
            let task_id = match self.pending_receiver.recv().await {
                Ok(task_id) => task_id,
                Err(_) => break
            };

            // Try to acquire a semaphore permit
            let permit = self.semaphore.clone().acquire_owned().await.unwrap();

            // Start the task
            let task = {
                if let Some(task) = self.tasks.get(&task_id) {
                    task.clone()
                } else {
                    drop(permit);
                    continue;
                }
            };

            let pending_sender = self.pending_sender.clone();
            let tasks_clone = self.tasks.clone();

            tokio::spawn(async move {
                let client = task.read().await.client.clone();
                let result = {
                    let state = task.read().await.state.clone();
                    download_task(state, client).await
                };

                let task_guard = task.read().await;
                let mut state_guard = task_guard.state.write().await;
                match result {
                    Ok(TaskStatus::Completed) => {
                        state_guard.status = TaskStatus::Completed;
                        info!("Download completed: {}", state_guard.file_path);
                    }
                    Ok(TaskStatus::Paused) => {
                        // Already to set Paused
                        info!("Task paused: {}", state_guard.file_path);
                        // Re-add to pending queue
                        // Let the next Pending task download
                        pending_sender.send(task_id).await.unwrap();
                    }
                    Ok(TaskStatus::Canceled) => {
                        state_guard.status = TaskStatus::Canceled;
                        info!("Task canceled: {}", state_guard.file_path);
                    }
                    Err(e) => {
                        state_guard.status = TaskStatus::Failed;
                        error!("Download failed: {}", e);
                    },
                    _ => {}
                }
            });

            // Release
            drop(permit);
        }
    }
}
