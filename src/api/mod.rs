use std::sync::Arc;
use axum::{Extension, Json, Router};
use axum::routing::{get, post};
use axum::extract::{Form};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::download::build_downloader;
use crate::download::config::Config;
use crate::download::download_task::DownloadTaskState;
use crate::download::downloader::Downloader;

/// Response data struct
#[derive(Serialize)]
struct HttpUserResponse {
    code: usize,
    message: String,
}

/// Parameters of control task
#[derive(Deserialize)]
struct TaskActionBody {
    uuid: Uuid,
}

/// Add task body
#[derive(Deserialize)]
struct AddTaskParams {
    url: String,
    filename: String
}

pub async fn setup_server() {
    let app = Router::new()
        .route("/task/list", get(task_list))
        .route("/task/add", post(add_task))
        .route("/task/pause", post(pause_task))
        .layer({
            let (downloader, scheduler) = build_downloader();
            tokio::spawn(async move {
                scheduler.run().await
            });
            Extension(Arc::new(downloader))
        })
        .layer(Extension(Arc::new(Config::default())));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:6060").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn task_list(Extension(downloader): Extension<Arc<Downloader>>) -> Json<Vec<DownloadTaskState>> {
    Json(downloader.get_tasks().await)
}

async fn add_task(Extension(config): Extension<Arc<Config>>, Extension(downloader): Extension<Arc<Downloader>>, Form(form): Form<AddTaskParams>) -> Json<HttpUserResponse> {
    let mut save_path = config.download_dir.clone();
    save_path.push(&form.filename);
    let save_path = save_path.to_str().unwrap().to_string();

    match downloader.add_task(form.url, save_path, config.chunk_size, config.retry_times).await {
        Ok(_) => Json(HttpUserResponse {
            code: 200,
            message: "Successfully added task!".to_string(),
        }),
        Err(_) => {
            Json(HttpUserResponse {
                code: 100,
                message: "Failed to add task!".to_string(),
            })
        }
    }
}

async fn pause_task(Extension(downloader): Extension<Arc<Downloader>>, Form(form): Form<TaskActionBody>) -> Json<HttpUserResponse> {
    downloader.pause_task(form.uuid).await;
    Json(HttpUserResponse {
        code: 200,
        message: "Successfully added task!".to_string(),
    })
}

mod tests {
    #[test]
    fn test_dir() {
        let mut v = dirs::config_dir().unwrap();
        v.push("downloader");
        println!("{:?}", v);
    }
}
