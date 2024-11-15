use crate::downloader::chunk_range::ChunkRange;

pub struct ChunkInfo {
    pub index: usize,
    pub range: ChunkRange
}