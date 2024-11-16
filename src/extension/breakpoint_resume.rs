///
/// 考虑两个点
/// 1. 如何在暂停或者宕机的时候把下载中任务状态保存下来
/// 2. 如何把保存的状态，恢复成任务，并从这个状态开始下载
///

use crate::extension::DownloaderExtension;

pub struct BreakpointResume {

}

impl DownloaderExtension for BreakpointResume {
    fn on_downloaded_len_change() {
        todo!()
    }

    fn on_file_pause() {
        todo!()
    }
}
