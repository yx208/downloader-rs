use crate::downloader::chunk_range::ChunkRange;

#[derive(Debug, Clone)]
pub struct ChunkInfo {
    pub index: usize,
    pub range: ChunkRange
}