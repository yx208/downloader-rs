use thiserror::Error;

#[derive(Error, Debug)]
pub enum DownloadStartError {

}

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("Http request failed, {:?}", .0)]
    HttpRequestError(#[from] reqwest::Error),
}

#[derive(Debug, Clone, Copy)]
pub enum DownloadEndCause {
    Finished,
    Canceled,
    Paused
}
