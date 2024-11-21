use std::collections::HashMap;
use std::num::{NonZeroU8, NonZeroUsize};
use url::Url;
use uuid::Uuid;
use crate::download::downloader::DownloaderConfig;
use crate::task::task::DownloadTask;

pub struct TaskManager {
    tasks: HashMap<Uuid, DownloadTask>
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new()
        }
    }

    pub fn add_task(&mut self) {
        let task = DownloadTask::new(Uuid::new_v4(), DownloaderConfig {
            retry_count: 3,
            url: Url::parse("https://tasset.xgy.tv/down/resources/agency/CeShiJiGou_1/QianDuanBuMen_2/dc9152119c160a601e5f684795fb4ea2_16/20241118/18154964fcaafce36522519013.mp4").unwrap(),
            save_dir: dirs::download_dir().unwrap(),
            file_name: "demo.mp4".to_string(),
            chunk_size: NonZeroUsize::new(1024 * 1024 * 4).unwrap(),
            concurrent: NonZeroU8::new(3).unwrap(),
        });
        self.tasks.insert(Uuid::new_v4(), task);
    }

    pub fn run_all(&mut self) {
    }
}
