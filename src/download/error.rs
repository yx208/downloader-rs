use thiserror::Error;
use tokio::io;

/// 下载中发生的错误
#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("Http request failed: {:?}", .0)]
    HttpRequestFailed(#[from] reqwest::Error),

    #[error("IOError: {:?}", .0)]
    IOError(#[from] io::Error),
}

/// 下载结束的原因
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadEndCause {
    Finished,
    Cancelled,
    Paused,
}

#[derive(Error, Debug)]
pub enum DownloadStartError {
    #[error("Open file failed: {:?}", .0)]
    OpenFileFailed(#[from] io::Error),

    #[error("Already downloading")]
    AlreadyDownloading,

    #[error("Directory does not exist")]
    DirectoryDoesNotExist,
}
