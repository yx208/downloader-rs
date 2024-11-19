#[derive(Debug, Clone)]
pub struct ChunkRange {
    pub start: u64,
    pub end: u64,
}

impl ChunkRange {
    pub fn new(start: u64, end: u64) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }

    pub fn from_len(start: u64, len: u64) -> Self {
        Self { start, end: start + len - 1 }
    }
    
    pub fn to_range_header(&self) -> headers::Range {
        headers::Range::bytes(self.start..self.end).unwrap()
    }
}
