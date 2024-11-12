use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkInfo {
    pub index: usize,
    pub range: ChunkRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkRange {
    pub start: u64,
    pub end: u64,
}

impl ChunkRange {
    pub fn new(start: u64, end: u64) -> ChunkRange {
        Self { start, end }
    }

    pub fn from_len(start: u64, len: u64) -> ChunkRange {
        Self { start, end: start + len - 1 }
    }

    pub fn to_range_header(&self) -> headers::Range {
        headers::Range::bytes(self).unwrap()
    }
    
    pub fn len(&self) -> u64 {
        (self.end - self.start) + 1
    }
}
