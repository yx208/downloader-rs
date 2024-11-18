use std::collections::Bound;
use std::ops::RangeBounds;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct ChunkRange {
    pub start: u64,
    pub end: u64,
}

impl ChunkRange {
    pub fn new(start: u64, end: u64) -> ChunkRange {
        ChunkRange { start, end }
    }

    pub fn from_len(start: u64, len: u64) -> ChunkRange {
        ChunkRange { start, end: start + len - 1 }
    }

    pub fn len(&self) -> u64 {
        (self.end - self.start) + 1
    }

    pub fn to_range_header(&self) -> headers::Range {
        headers::Range::bytes(self).unwrap()
    }
    
    pub fn clone_with_offset(&self, offset: u64) -> ChunkRange {
        ChunkRange {
            start: self.start + offset,
            end: self.end
        }
    }
}

/// 为 ChunkRange 实现范围 trait: start..end
impl<'a> RangeBounds<u64> for &'a ChunkRange {
    fn start_bound(&self) -> Bound<&u64> {
        Bound::Included(&self.start)
    }

    fn end_bound(&self) -> Bound<&u64> {
        Bound::Included(&self.end)
    }
}

