use crate::download::chunk_range::ChunkRange;

pub struct ChunkIterator {
    chunk_range: ChunkRange
}

impl ChunkIterator {
    pub fn new(chunk_range: ChunkRange) -> Self {
        Self { chunk_range }
    }
}
