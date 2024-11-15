use std::num::NonZeroUsize;
use crate::downloader::chunk_range::ChunkRange;

/// 迭代器不断从这里取出 chunk 进行下载
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
pub struct ChunkIteratorState {
    pub remaining: RemainingChunks
}

pub struct ChunkIterator {
    pub data: ChunkIteratorState,
}

impl ChunkIterator {
    pub fn new(data: ChunkIteratorState) -> Self {
        Self {
            data
        }
    }
}

impl Iterator for ChunkIterator {
    type Item = ChunkRange;

    fn next(&mut self) -> Option<Self::Item> {
        self.data.remaining.take_range()
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

    #[tokio::test]
    async fn should_be_run() {
        let remaining= RemainingChunks::new(NonZeroUsize::new(1).unwrap(), 0);
        let state = ChunkIteratorState { remaining };
        let iter = ChunkIterator::new(state);
    }
}
