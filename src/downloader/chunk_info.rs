use serde::{Deserialize, Serialize};
use crate::downloader::chunk_range::ChunkRange;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkInfo {
    pub index: usize,
    pub range: ChunkRange
}