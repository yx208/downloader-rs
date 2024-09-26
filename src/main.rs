mod download;
mod utils;

use std::env;
use std::sync::Arc;
use anyhow::Result;
use log::{error, info};
use crate::download::config::Config;
use crate::download::download_task::TaskStatus;
use crate::download::downloader::Downloader;
use crate::download::logger;

#[tokio::main]
async fn main() -> Result<()> {
    // No log file by default.
    logger::setup_logger()?;

    // Load configuration.
    let config_file = env::args().nth(1).unwrap_or_else(|| "config.json".to_string());
    let config = Config::load_from_file(&config_file).unwrap_or_else(|_| Config::default());

    // If the directory does not exist, it will be created.
    std::fs::create_dir_all(&config.download_dir)?;

    let state_file = "download_state.json";
    let downloader = Downloader::new(config.max_concurrent_downloads, state_file.to_string());
    downloader.load_state().await?;

    // Load task from configuration
    for task_config in config.tasks {
        let url = task_config.url;
        let file_name = task_config.file_name.unwrap_or_else(|| {
            url.split('/').last().unwrap_or("download").to_string()
        });
        let file_path = format!("{}/{}", config.download_dir, file_name);
        downloader.add_task(url.clone(), file_path, config.chunk_size, config.retry_times).await?;
    }

    let downloader_arc = Arc::new(downloader);

    loop {
        let tasks = downloader_arc.get_tasks().await;
        if tasks.iter().all(|task| task.status == TaskStatus::Completed || task.status == TaskStatus::Failed) {
            info!("All tasks have been completed");
            break;
        }

        for task in tasks {
            info!(
                "Task: {} - status: {:?} - Progress: {:.2}%",
                task.file_path,
                task.status,
                task.downloaded as f64 / task.file_size as f64 * 100.0
            );
        }

        downloader_arc.save_state()
            .await
            .unwrap_or_else(|e| error!("Save state failed: {}", e));

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    downloader_arc.save_state().await?;

    Ok(())
}
