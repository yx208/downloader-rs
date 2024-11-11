use anyhow::Result;

pub trait DownloaderWrapper: Send + Sync + 'static {
    fn prepare_download() -> Result<(), ()> {
        Ok(())
    }
}

pub struct Extension {}

impl DownloaderWrapper for Extension {
    fn prepare_download() -> Result<(), ()> {
        Ok(())
    }
}
