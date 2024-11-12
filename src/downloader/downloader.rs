#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    HttpRequestFailed(#[from] reqwest::Error)
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum DownloadingEndCause {
    DownloadFinished,
    Cancelled,
    Paused
}
