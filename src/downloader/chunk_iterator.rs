use std::num::NonZeroUsize;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use crate::downloader::chunk_info::ChunkInfo;
use crate::downloader::chunk_range::ChunkRange;

/// 迭代器不断从这里取出 chunk 进行下载
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemainingChunks {
    chunk_size: usize,
    ranges: Vec<ChunkRange>
}

impl RemainingChunks {
    pub fn new(chunk_size: NonZeroUsize, file_size: u64) -> Self {
        Self {
            chunk_size: chunk_size.get(),
            ranges: vec![ChunkRange::from_len(0, file_size)]
        }
    }

    pub fn take_range(&mut self) -> Option<ChunkRange> {
        let chunk_size = self.chunk_size as u64;
        match self.ranges.first().map(|range| range.to_owned()) {
            None => None,
            Some(range) => {
                let length = match range.len() {
                    0 => {
                        self.ranges.remove(0);
                        return self.take_range();
                    }
                    // 最后一个 chunk
                    len if len <= chunk_size => {
                        self.ranges.remove(0);
                        len
                    }
                    // 往后推一个 chunk
                    _ => {
                        self.ranges[0] = ChunkRange::new(range.start + chunk_size, range.end);
                        chunk_size
                    }
                };

                Some(ChunkRange::from_len(range.start, length))
            }
        }
    }
}

/// 存储 chunk 状态
#[derive(Debug, Clone)]
pub struct ChunkIteratorData {
    // 计数产生了多少个 chunk
    pub iter_count: usize,
    pub remaining: RemainingChunks
}

impl ChunkIteratorData {
    /// 计算仍需下载多少
    pub fn remaining_len(&self) -> u64 {
        let mut len = 0;
        for item in &self.remaining.ranges {
            len += item.len();
        }

        len
    }

    pub fn next_chunk_range(&mut self) -> Option<ChunkInfo> {
        let range = self.remaining.take_range();
        if let Some(range) = range {
            self.iter_count += 1;
            Some(ChunkInfo {
                index: self.iter_count,
                range
            })
        } else {
            None
        }
    }
}

pub struct ChunkIterator {
    pub data: Arc<parking_lot::RwLock<ChunkIteratorData>>,
}

impl ChunkIterator {
    pub fn new(data: ChunkIteratorData) -> Self {
        Self {
            data: Arc::new(parking_lot::RwLock::new(data))
        }
    }

    pub fn next(&self) -> Option<ChunkInfo> {
        let mut data = self.data.write();
        data.next_chunk_range()
    }
}

mod tests {
    use super::*;

    #[test]
    fn remaining_should_be_next() {
        impl PartialEq<Self> for ChunkRange {
            fn eq(&self, other: &Self) -> bool {
                self.start == other.start && self.end == other.end
            }
        }

        let chunk_size = NonZeroUsize::new(100).unwrap();
        let file_size = 100 * 3;
        let mut remaining= RemainingChunks::new(chunk_size, file_size);

        let range = remaining.take_range().unwrap();
        assert_eq!(range, ChunkRange::new(0, 99));

        let range = remaining.take_range().unwrap();
        assert_eq!(range, ChunkRange::new(100, 199));

        let range = remaining.take_range().unwrap();
        println!("{:?}", range.len());
        // 从零开始，299 即是 300
        assert_eq!(range, ChunkRange::new(200, 299));

        let range = remaining.take_range();
        assert!(range.is_none());

        assert!(remaining.ranges.is_empty());
    }
}
