use std::sync::Arc;
use axum::{Extension, Json, Router};
use axum::routing::get;

use crate::download::config::Config;
use crate::download::download_task::DownloadTaskState;
use crate::download::downloader::Downloader;

pub async fn setup_server() {
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
            String::from("https://example.com/large_file.zip"),
            String::from("/save/filename.zip"),
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
