///
/// 添加进就开始下载（考虑配置化）
///

use uuid::Uuid;
use crate::extension::DownloaderExtension;
use crate::task::task::DownloadTask;

pub struct TaskManager {
    extensions: Vec<u8>,
    tasks: Vec<DownloadTask>
}

impl TaskManager {
    pub fn new() -> TaskManager {
        Self {
            tasks: Vec::new(),
            extensions: Vec::new()
        }
    }

    pub fn resume(&self) {

    }

    pub fn pause(&self, task_id: Uuid) {

    }

    pub fn run_all(&self) {

    }

    pub fn add_task(&mut self, task: DownloadTask) -> Uuid {
        self.tasks.push(task);
        Uuid::new_v4()
    }
}

mod tests {
    use super::*;

    #[tokio::test]
    async fn should_be_run() {
        let mut manager = TaskManager::new();
    }
}
