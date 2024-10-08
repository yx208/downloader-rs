use std::sync::Arc;
use dashmap::DashMap;
use async_channel::{unbounded};
use tokio::sync::{Semaphore};

pub mod config;
pub mod logger;
pub mod downloader;
pub mod persistence;
pub mod download_task;
pub mod scheduler;

pub fn build_downloader() -> (downloader::Downloader, scheduler::Scheduler) {
    let tasks = Arc::new(DashMap::new());
    let (pending_sender, pending_receiver) = unbounded();
    let semaphore = Arc::new(Semaphore::new(3));

    let downloader = downloader::Downloader::new(tasks.clone(), pending_sender.clone());
    let scheduler = scheduler::Scheduler::new(
        tasks.clone(),
        semaphore.clone(),
        pending_receiver,
        pending_sender.clone(),
    );

    (downloader, scheduler)
}

mod tests {
    use std::sync::Arc;
    use std::time::Duration;
    use async_channel::unbounded;
    use dashmap::DashMap;
    use tokio::sync::Semaphore;
    use log::{info};
    use super::*;

    use crate::download::config::Config;
    use crate::download::download_task::{DownloadTaskState, TaskStatus};

    fn print_progress(tasks: &Vec<DownloadTaskState>) {
        for task in tasks {
            info!(
                "Task [{}]: {} - Status: {:?} - Progress: {:.2}%",
                task.id,
                task.file_path,
                task.status,
                if task.file_size > 0 {
                    task.downloaded as f64 / task.file_size as f64 * 100.0
                } else {
                    0.0
                }
            );
        }
    }

    #[tokio::test]
    async fn should_be_run() {
        let config = Config::default();
        let (downloader, scheduler) = build_downloader();

        tokio::spawn(async move {
            scheduler.run().await;
        });

        let urls = vec![
            vec!["https://oss.example.com/cool.mp4", "cool.mp4"],
            vec!["https://oss.example.com/cool.mp4", "hot.mp4"],
        ];

        for url in urls {
            let file_url = url[0].to_string();
            let file_name = url[1].to_string();
            let mut file_path = config.download_dir.clone();
            file_path.push(file_name);
            let file_path = file_path.to_str().unwrap().to_string();
            downloader.add_task(file_url, file_path, config.chunk_size, config.retry_times).await.unwrap();
        }

        loop {
            let tasks = downloader.get_tasks().await;
            if tasks.is_empty() {
                info!("No tasks running");
                break;
            }

            print_progress(&tasks);

            downloader.remove_completed_tasks().await;

            if tasks.iter().all(|t| t.status == TaskStatus::Completed) {
                info!("All tasks completed");
                break;
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}
