use std::collections::Bound;
use std::num::NonZeroUsize;
use std::ops::RangeBounds;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ChunkInfo {
    pub index: usize,
    pub range: ChunkRange,
}

#[derive(Debug, Clone, Copy, PartialEq)]
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

pub struct ChunkData {
    iter_count: usize,
    chunk_size: usize,
    // 文件还剩余下载的部分范围
    remaining: Option<ChunkRange>,
    last_incomplete_chunks: Vec<ChunkInfo>,
}

impl ChunkData {
    pub fn new(chunk_size: NonZeroUsize, content_length: u64) -> Self {
        Self {
            iter_count: 0,
            chunk_size: chunk_size.get(),
            remaining: Some(ChunkRange::from_len(0, content_length)),
            last_incomplete_chunks: Vec::new(),
        }
    }

    pub fn next(&mut self) -> Option<ChunkInfo> {
        let chunk_size = self.chunk_size as u64;
        match &self.remaining {
            None => None,
            Some(remaining) => {
                let remaining = remaining.clone();
                let len = match remaining.len() {
                    0 => {
                        return None;
                    }
                    // 最后一个 chunk 的大小
                    len if len <= chunk_size => {
                        self.remaining = None;
                        len
                    },
                    // 大于一个 chunk 的情况
                    _ => {
                        self.remaining = Some(ChunkRange::new(remaining.start + chunk_size, remaining.end));
                        chunk_size
                    }
                };

                self.iter_count += 1;
                Some(ChunkInfo {
                    index: self.iter_count,
                    range: ChunkRange::from_len(remaining.start, len)
                })
            }
        }
    }
}

pub struct ChunkRangeIterator {
    chunk_data: Arc<parking_lot::RwLock<ChunkData>>
}

impl ChunkRangeIterator {
    pub fn new(chunk_data: ChunkData) -> Self {
        Self {
            chunk_data: Arc::new(
                parking_lot::RwLock::new(chunk_data)
            )
        }
    }

    pub fn next(&self) -> Option<ChunkInfo> {
        let mut chunk_data = self.chunk_data.write();
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

