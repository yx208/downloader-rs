mod download;
mod cli;

use std::sync::Arc;
use axum::routing::{get};
use axum::{Extension, Router, Json};

use crate::download::config::Config;
use crate::download::download_task::DownloadTaskState;
use crate::download::downloader::Downloader;

#[tokio::main]
async fn main() {
    let config = Arc::new(Config::default());
    let downloader = Downloader::new_default();
    let app = Router::new()
        .route("/task/add", get(add_task))
        .route("/task/list", get(task_list))
        .layer(Extension(config))
        .layer(Extension(downloader));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:6060").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn add_task(Extension(config): Extension<Arc<Config>>, Extension(downloader): Extension<Arc<Downloader>>) -> String {
    downloader
        .add_task(
            String::from("https://oss.xgy.tv/xgy/design/test/cool.mp4"),
            String::from("c:/Users/X/Downloads/demo.mp4"),
            config.chunk_size,
            config.retry_times
        )
        .await
        .unwrap();

    String::from("Added task")
}

async fn task_list(Extension(downloader): Extension<Arc<Downloader>>) -> Json<Vec<DownloadTaskState>> {
    Json(downloader.get_tasks().await)
}
