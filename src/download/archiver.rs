use serde::{Deserialize, Serialize};
use crate::download::chunk_range::ChunkData;

#[derive(Serialize, Deserialize)]
pub struct Archiver {
    pub downloaded_len: u64,
    pub chunk_data: Option<ChunkData>
}
