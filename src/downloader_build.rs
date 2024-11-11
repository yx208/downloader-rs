use crate::downloader::{ExtendedHttpFileDownloader, HttpFileDownloader};
use crate::extension::{DownloaderWrapper};

pub struct DownloaderBuild {}

impl DownloaderBuild {
    pub fn new() -> Self {
        Self {}
    }

    pub fn build() -> (ExtendedHttpFileDownloader, ())  {
        let downloader = HttpFileDownloader {};

        let wrapper = Box::new((DownloaderWrapper {}));
        (ExtendedHttpFileDownloader::new(downloader, wrapper), ())
    }
}

