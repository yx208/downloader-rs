mod speed_tracker;

use anyhow::Result;
use crate::downloader::downloader::HttpFileDownloader;

pub trait DownloaderWrapper: Send + Sync + 'static {
    fn prepare_download() -> Result<(), ()> {
        Ok(())
    }
}

pub trait DownloadExtensionBuilder: 'static {
    type Wrapper: DownloaderWrapper;
    type ExtensionState;

    fn build(self, downloader: &mut HttpFileDownloader) -> (Self::Wrapper, Self::ExtensionState) where Self: Sized;
}

