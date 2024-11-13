
/// 跟踪下载速度
#[derive(Default)]
pub struct DownloadSpeedTrackerExtension {}

impl DownloadSpeedTrackerExtension {
    pub fn new() -> Self {
        Self::default()
    }
}
