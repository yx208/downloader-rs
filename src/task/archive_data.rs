use serde::{Deserialize, Serialize};
use crate::downloader::chunk_info::ChunkInfo;
use crate::downloader::chunk_iterator::RemainingChunks;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DownloadArchiveData {
    pub downloaded_len: u64,
    pub remaining_chunks: RemainingChunks,
    pub last_incomplete_chunks: Vec<ChunkInfo>
}

pub struct Archive {

}
