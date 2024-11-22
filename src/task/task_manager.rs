use std::collections::HashMap;
use url::Url;
use uuid::Uuid;
use tokio::sync::mpsc;
use crate::download::downloader::DownloaderConfig;
use crate::task::task::DownloadTask;

pub struct TaskManager {
    completed_notify_sender: mpsc::Sender<Uuid>,
    completed_notify_receiver: mpsc::Receiver<Uuid>,
    tasks: HashMap<Uuid, DownloadTask>
}

impl TaskManager {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(32);

        Self {
            completed_notify_sender: tx,
            completed_notify_receiver: rx,
            tasks: HashMap::new()
        }
    }

    pub fn add_task(&mut self) {
        let url = Url::parse("https://tasset.xgy.tv/down/resources/agency/CeShiJiGou_1/QianDuanBuMen_2/dc9152119c160a601e5f684795fb4ea2_16/20241118/18154964fcaafce36522519013.mp4").unwrap();
        let task = DownloadTask::new(Uuid::new_v4(), DownloaderConfig::from_url(url));
        self.tasks.insert(Uuid::new_v4(), task);
    }
}
