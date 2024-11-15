use std::num::NonZeroUsize;
use crate::downloader::chunk_iterator::{ChunkIterator, ChunkIteratorState, RemainingChunks};
use crate::downloader::chunk_manager::ChunkManger;
use crate::downloader::download_way::DownloadWay;

pub struct FileDownloader {
    download_way: DownloadWay
}

impl FileDownloader {
    pub fn new() -> Self {
        let remaining = RemainingChunks::new(NonZeroUsize::new(1024).unwrap(), 1024);
        let state = ChunkIteratorState { remaining };
        let iter = ChunkIterator::new(state);
        let chunk_manager = ChunkManger::new(iter);

        Self {
            download_way: DownloadWay::Range(chunk_manager)
        }
    }
}
