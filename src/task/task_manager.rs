use crate::download::downloader::Downloader;

pub struct TaskManager {
    tasks: Vec<Downloader>
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new()
        }
    }

    pub fn add_task(&mut self) {

    }

    pub fn run_all(&mut self) {
        let mut futures = Vec::new();
        for downloader in self.tasks.iter_mut() {
            let future = downloader.download();
            futures.push(future);
        }
    }
}
