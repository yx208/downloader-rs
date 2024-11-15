use std::num::{NonZeroU8, NonZeroUsize};
use crate::downloader::chunk_iterator::{ChunkIterator, ChunkIteratorData, RemainingChunks};
use crate::downloader::chunk_manager::ChunkManger;
use crate::downloader::download_way::DownloadWay;

pub struct FileDownloader {
    download_way: DownloadWay
}

impl FileDownloader {
    pub fn new() -> Self {
        let remaining = RemainingChunks::new(NonZeroUsize::new(1024).unwrap(), 1024);
        let state = ChunkIteratorData { iter_count: 0, remaining };
        let iter = ChunkIterator::new(state);
        let chunk_manager = ChunkManger::new(
            NonZeroU8::new(3).unwrap(),
            3,
            iter,
            Default::default(),
            Default::default()
        );

        Self {
            download_way: DownloadWay::Range(chunk_manager)
        }
    }
}
