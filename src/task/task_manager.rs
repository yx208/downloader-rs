use std::collections::HashMap;
use std::sync::Arc;
use url::Url;
use uuid::Uuid;
use tokio::sync::{Mutex};
use crate::download::downloader::DownloaderConfig;
use crate::download::error::DownloadEndCause;
use crate::task::task::{DownloadStatus, DownloadTask};

pub struct TaskManager {
    tasks: HashMap<Uuid, Arc<Mutex<DownloadTask>>>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new()
        }
    }

    pub fn add_task(&mut self) {
        let url = Url::parse("https://tasset.xgy.tv/down/resources/agency/CeShiJiGou_1/QianDuanBuMen_2/dc9152119c160a601e5f684795fb4ea2_16/20241118/18154964fcaafce36522519013.mp4").unwrap();
        let id = Uuid::new_v4();
        let downloader = DownloadTask::new(id.clone(), DownloaderConfig::from_url(url));
        let task = Arc::new(Mutex::new(downloader));
        self.tasks.insert(id.clone(), task.clone());

        tokio::spawn(async move {
            let future = {
                let mut guard= task.lock().await;
                guard.download().await.unwrap()
            };

            let cause = future.await.unwrap();
            if cause == DownloadEndCause::Finished {
                let mut guard = task.lock().await;
                guard.change_status(DownloadStatus::Finished);
            }
        });
    }
}

mod tests {
    use super::*;

    #[tokio::test]
    async fn should_be_run() {
        let mut manager = TaskManager::new();
        manager.add_task();
    }
}

