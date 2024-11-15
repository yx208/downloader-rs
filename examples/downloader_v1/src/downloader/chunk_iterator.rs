use std::collections::Bound;
use std::num::NonZeroUsize;
use std::ops::RangeBounds;
use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// 要对 chunk 做的操作
enum ChunkHandle {
    Update {
        index: usize,
        range: ChunkRange
    },
    Split {
        index: usize,
        left: ChunkRange,
        right: ChunkRange
    },
    Remove(usize),
    Error,
}

/// 记录每块大小，以及还有哪些 range 块
#[derive(Debug, Clone)]
pub struct RemainingChunks {
    pub chunk_size: usize,
    pub ranges: Vec<ChunkRange>
}

impl RemainingChunks {
    pub fn new(chunk_size: NonZeroUsize, total_len: u64) -> Self {
        Self {
            chunk_size: chunk_size.get(),
            // 这里生成一个总体的 chunk
            ranges: vec![ChunkRange::from_len(0, total_len)]
        }
    }

    pub fn take_first(&mut self) -> Option<ChunkRange> {
        let chunk_size = self.chunk_size as u64;
        // 取出第一个 range，并创建所有权副本
        let mapped_range = self.ranges.first().map(|n| n.to_owned());
        match mapped_range {
            // 没有 Chunk 了
            None => None,
            Some(range) => {
                let length = match range.len() {
                    // 没有长度，继续向下获取，并把这个移除
                    0 => {
                        self.handle(ChunkHandle::Remove(0));
                        return self.take_first();
                    },
                    // 获取到正确的长度，创建一个 range 副本，并把这个 range 移除
                    len if len <= chunk_size => {
                        self.handle(ChunkHandle::Remove(0));
                        len
                    },
                    // 其他情况
                    _ => {
                        self.handle(ChunkHandle::Update {
                            index: 0,
                            range: ChunkRange::new(range.start + chunk_size, range.end),
                        });

                        chunk_size
                    }
                };

                Some(ChunkRange::from_len(range.start, length))
            }
        }
    }

    fn handle(&mut self, handle: ChunkHandle) -> bool {
        match handle {
            ChunkHandle::Update { index, range } => {
                self.ranges[index] = range;
            }
            ChunkHandle::Split { index, left, right } => {
                self.ranges[index] = right;
                self.ranges.insert(index, left);
            }
            ChunkHandle::Remove(index) => {
                self.ranges.remove(index);
            }
            ChunkHandle::Error => {
                return false;
            }
        }

        true
    }
}

#[derive(Debug, Clone)]
pub struct ChunkData {
    // 获取了多少次 next range
    pub iter_count: usize,
    pub remaining: RemainingChunks,
    // 最后未完成的块
    pub last_incomplete_chunks: Vec<ChunkInfo>
}

impl ChunkData {
    pub fn next_chunk_range(&mut self) -> Option<ChunkInfo> {
        // 如果有未完成的，则先使用
        if let Some(chunk) = self.last_incomplete_chunks.pop() {
            println!("pop");
            return Some(chunk);
        }

        let range = self.remaining.take_first();
        if let Some(range) = range {
            self.iter_count += 1;
            let result = ChunkInfo {
                index: self.iter_count,
                range
            };

            Some(result)
        } else {
            // 没有 Chunk 了
            None
        }
    }
}

/// 用于迭代上传 chunk
pub struct ChunkIterator {
    pub content_length: u64,
    pub data: Arc<RwLock<ChunkData>>
}

impl ChunkIterator {
    pub fn new(content_length: u64, data: ChunkData) -> Self {
        Self {
            content_length,
            data: Arc::new(RwLock::new(data))
        }
    }

    pub fn next(&self) -> Option<ChunkInfo> {
        let mut data = self.data.write();
        data.next_chunk_range()
    }
}

/// 迭代的 chunk 信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkInfo {
    // chunk 索引
    pub index: usize,
    // 数据 range
    pub range: ChunkRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkRange {
    pub start: u64,
    pub end: u64,
}

impl ChunkRange {
    pub fn new(start: u64, end: u64) -> Self {
        Self { start, end }
    }

    pub fn from_len(start: u64, len: u64) -> Self {
        Self { start, end: start + len - 1 }
    }

    pub fn to_range_header(&self) -> headers::Range {
        headers::Range::bytes(self).unwrap()
    }
    
    pub fn len(&self) -> u64 {
        (self.end - self.start) + 1
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

mod tests {
    use std::num::NonZeroUsize;
    use crate::downloader::chunk_iterator::{ChunkData, ChunkInfo, ChunkIterator, ChunkRange, RemainingChunks};

    #[tokio::test]
    async fn test_remaining_chunks() {
        let chunk_size = NonZeroUsize::new(1024 * 1024 * 4).unwrap();
        let content_length = 40_593_263;
        let mut remaining_chunks = RemainingChunks::new(chunk_size, content_length);

        let chunk = remaining_chunks.take_first();
        println!("{:?}", remaining_chunks);

        match chunk {
            None => {}
            Some(chunk) => {
                println!("{:?}", chunk);
            }
        }
    }

    #[tokio::test]
    async fn test_range() {
        let chunk_size: u64 = 1024 * 1024 * 4;
        let content_length = 40_593_263;
        let chunk_data = ChunkData {
            iter_count: 0,
            remaining: RemainingChunks::new(NonZeroUsize::new(chunk_size as usize).unwrap(), content_length),
            last_incomplete_chunks: Vec::new()
        };
        let iter = ChunkIterator::new(chunk_size, chunk_data);
    }
}
