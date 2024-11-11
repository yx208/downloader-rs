use crate::extension::{DownloaderWrapper};

pub struct HttpFileDownloader {}

pub struct ExtendedHttpFileDownloader {
    pub inner: HttpFileDownloader,
    downloader_wrapper: Box<dyn (DownloaderWrapper)>
}

impl ExtendedHttpFileDownloader {
    pub fn new(downloader: HttpFileDownloader, wrapper: Box<dyn (DownloaderWrapper)>) -> Self {
        Self {
            inner: downloader,
            downloader_wrapper: wrapper
        }
    }
}
