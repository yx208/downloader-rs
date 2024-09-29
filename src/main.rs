// main.rs

mod download;
mod cli;

use anyhow::Result;
use log::{error, info};
use std::sync::Arc;
use tokio::signal;

use download::logger;
use download::config::Config;
use download::downloader::Downloader;
use download::download_task::{TaskStatus};

#[tokio::main]
async fn main() -> Result<()> {
    // 设置日志
    logger::setup_logger()?;

    // 加载配置
    let mut download_dir = dirs::download_dir().unwrap();
    download_dir.push("config.json");
    let config = Config::load_from_file(download_dir.to_str().unwrap())?;

    // 创建下载目录（如果不存在）
    tokio::fs::create_dir_all(&config.download_dir).await?;

    // 初始化下载器
    let state_file = "download_state.json";
    let downloader = Arc::new(Downloader::new(
        config.max_concurrent_downloads,
        state_file.to_string(),
    ));

    // 加载之前的状态
    downloader.load_state().await?;

    // 从配置中添加任务
    for task_config in config.tasks {
        let url = task_config.url.clone();
        let file_name = task_config.file_name.unwrap_or_else(|| {
            url.split('/')
                .last()
                .unwrap_or("download")
                .to_string()
        });
        let file_path = format!("{}/{}", config.download_dir, file_name);
        downloader.add_task(url.clone(), file_path, config.chunk_size, config.retry_times).await?;
    }

    // 监听 Ctrl+C 信号，用于优雅退出
    let downloader_clone = downloader.clone();
    tokio::spawn(async move {
        signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        info!("Received interrupt signal, saving state...");
        downloader_clone
            .save_state()
            .await
            .unwrap_or_else(|e| error!("Failed to save state: {}", e));
        std::process::exit(0);
    });

    // 简单的进度显示，每隔 5 秒输出一次
    loop {
        let tasks = downloader.get_tasks().await;
        if tasks.is_empty() {
            info!("No tasks running");
            break;
        }

        for task in &tasks {
            info!(
                "Task [{}]: {} - Status: {:?} - Progress: {:.2}%",
                task.id,
                task.file_path,
                task.status,
                task.downloaded as f64 / task.file_size as f64 * 100.0
            );
        }

        downloader
            .save_state()
            .await
            .unwrap_or_else(|e| error!("Failed to save state: {}", e));

        // 移除已完成的任务
        downloader.remove_completed_tasks().await;

        // 检查是否所有任务都已完成
        if tasks.iter().all(|t| t.status == TaskStatus::Completed || t.status == TaskStatus::Failed || t.status == TaskStatus::Canceled) {
            info!("All tasks completed");
            break;
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }

    // 最后保存状态
    downloader.save_state().await?;

    Ok(())
}
