use std::sync::Arc;
use crate::downloader::chunk_manager::ChunkManager;

struct SingleDownload {}

pub enum DownloadWay {
    Ranges(Arc<ChunkManager>),
    Single(SingleDownload),
}