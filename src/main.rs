mod download;
mod cli;

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
    let mut config_file = dirs::download_dir().unwrap();
    config_file.push("config.json");
    let config = Config::load_from_file(config_file.to_str()
        .unwrap())
        .unwrap_or_else(|_| Config::default());

    // If the directory does not exist, it will be created.
    tokio::fs::create_dir_all(&config.download_dir).await?;

    // 定义状态保存文件
    let mut state_file = dirs::download_dir().unwrap();
    state_file.push("download_state.json");

    // 下载器
    let downloader = Arc::new(
        Downloader::new(config.max_concurrent_downloads, state_file.to_str().unwrap().to_string())
    );
    downloader.load_state().await?;

    // Load task from configuration
    for task_config in config.tasks {
        let url = task_config.url.clone();
        let file_name = task_config.file_name.unwrap_or_else(|| {
            url.split('/').last().unwrap_or("download").to_string()
        });
        let file_path = format!("{}/{}", config.download_dir, file_name);
        downloader
            .add_task(url.clone(), file_path, config.chunk_size, config.retry_times)
            .await
            .unwrap_or_else(|e| error!("Failed to add task {}: {}", file_name, e));
    }

    let downloader_clone = Arc::clone(&downloader);
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Failed to listen for event");
        info!("Received interrupt signal, saving status. ...");
        downloader_clone
            .save_state()
            .await
            .unwrap_or_else(|e| error!("Failed to save state: {}", e));
        std::process::exit(0);
    });

    // 每隔一秒显示进度与速度
    loop {
        let tasks = downloader.get_tasks().await;
        if tasks.is_empty() {
            info!("Nothing to download");
            break;
        }

        for task in &tasks {
            info!(
                "任务：{} - 状态：{:?} - 进度：{:.2}%",
                task.file_path,
                task.status,
                task.downloaded as f64 / task.file_size as f64 * 100.0
            );
        }

        downloader
            .save_state()
            .await
            .unwrap_or_else(|e| error!("Failed to save state: {}", e));

        // Check all task is completed
        if tasks.iter().all(|t| t.status == TaskStatus::Completed || t.status == TaskStatus::Failed) {
            info!("All task have been completed");
            break;
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    Ok(())
}
