use std::collections::Bound;
use std::num::NonZeroUsize;
use std::ops::RangeBounds;
use std::sync::Arc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkInfo {
    pub index: usize,
    pub range: ChunkRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ChunkRange {
    pub start: u64,
    pub end: u64,
}

impl ChunkRange {
    pub fn new(start: u64, end: u64) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> u64 {
        (self.end - self.start) + 1
    }

    pub fn from_len(start: u64, len: u64) -> Self {
        Self { start, end: start + len - 1 }
    }
    
    pub fn to_range_header(&self) -> headers::Range {
        headers::Range::bytes(self).unwrap()
    }
}

impl<'a> RangeBounds<u64> for &'a ChunkRange {
    fn start_bound(&self) -> Bound<&u64> {
        Bound::Included(&self.start)
    }

    fn end_bound(&self) -> Bound<&u64> {
        Bound::Included(&self.end)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RemainingChunks {
    chunk_size: usize,
    ranges: Vec<ChunkRange>
}

impl RemainingChunks {
    pub fn new(chunk_size: NonZeroUsize, content_length: u64) -> Self {
        Self {
            chunk_size: chunk_size.get(),
            ranges: vec![ChunkRange::from_len(0, content_length)]
        }
    }

    pub fn next(&mut self) -> Option<ChunkRange> {
        let chunk_size = self.chunk_size as u64;
        match self.ranges.first().map(|range| range.to_owned()) {
            None => None,
            Some(range) => {
                let len = match range.len() {
                    0 => {
                        self.ranges.remove(0);
                        return self.next();
                    }
                    len if len <= chunk_size => {
                        self.ranges.remove(0);
                        len
                    }
                    _ => {
                        self.ranges[0] = ChunkRange::new(range.start + chunk_size, range.end);
                        chunk_size
                    }
                };

                Some(ChunkRange::from_len(range.start, len))
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChunkData {
    pub iter_count: usize,
    // 文件还剩余下载的部分范围
    pub remaining: RemainingChunks,
    pub last_incomplete_chunks: Vec<ChunkInfo>,
}

impl ChunkData {
    pub fn new(chunk_size: NonZeroUsize, content_length: u64) -> Self {
        Self {
            iter_count: 0,
            remaining: RemainingChunks::new(chunk_size, content_length),
            last_incomplete_chunks: Vec::new(),
        }
    }

    /// 获取剩余的字节
    pub fn remaining_len(&self) -> u64 {
        let mut len = 0;

        for range in &self.remaining.ranges {
            len += range.len();
        }

        for info in &self.last_incomplete_chunks {
            len += info.range.len();
        }

        len
    }

    pub fn next(&mut self) -> Option<ChunkInfo> {
        if let Some(chunk_info) = self.last_incomplete_chunks.pop() {
            return Some(chunk_info);
        }

        let range = self.remaining.next();
        if let Some(range) = range {
            self.iter_count += 1;

            let result = ChunkInfo {
                index: self.iter_count,
                range
            };

            println!("{:?}", result);

            Some(result)
        } else {
            None
        }
    }
}

pub struct ChunkRangeIterator {
    pub data: Arc<parking_lot::RwLock<ChunkData>>
}

impl ChunkRangeIterator {
    pub fn new(chunk_data: ChunkData) -> Self {
        Self {
            data: Arc::new(
                parking_lot::RwLock::new(chunk_data)
            )
        }
    }

    pub fn next(&self) -> Option<ChunkInfo> {
        let mut chunk_data = self.data.write();
        chunk_data.next()
    }
}

mod tests {
    use super::*;

    fn create_iter() -> ChunkRangeIterator {
        // let url = Url::parse("http://localhost:23333/image.jpg").unwrap();
        // let content_length = get_file_length(url);
        let chunk_size = NonZeroUsize::new(100).unwrap();
        let test_length = chunk_size.get() * 3;
        let chunk_data = ChunkData::new(chunk_size, test_length as u64);
        let iter = ChunkRangeIterator::new(chunk_data);

        iter
    }

    #[tokio::test]
    async fn should_be_next() {
        let mut iter = create_iter();

        let next = iter.next();
        assert!(next.is_some());
        let next = next.unwrap();
        assert_eq!(next.index, 1);
        assert_eq!(next.range, ChunkRange::from_len(0, 100));

        let next = iter.next();
        assert!(next.is_some());
        let next = next.unwrap();
        assert_eq!(next.index, 2);
        assert_eq!(next.range, ChunkRange::from_len(100, 100));

        let next = iter.next();
        assert!(next.is_some());
        let next = next.unwrap();
        assert_eq!(next.index, 3);
        assert_eq!(next.range, ChunkRange::from_len(200, 100));

        assert!(iter.next().is_none());
    }
}

