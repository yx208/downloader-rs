use tokio::fs::File;
use std::sync::Arc;
use anyhow::Result;
use reqwest::Response;
use crate::downloader::chunk_manager::ChunkManager;
use crate::downloader::downloader::{DownloadError, DownloadingEndCause};

pub struct SingleDownload {}

impl SingleDownload {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn download(
        &self,
        mut file: File,
        response: Box<Response>,
        buffer_size: usize
    ) -> Result<DownloadingEndCause, DownloadError> {
        todo!()
    }
}

pub enum DownloadWay {
    Ranges(Arc<ChunkManager>),
    Single(SingleDownload),
}