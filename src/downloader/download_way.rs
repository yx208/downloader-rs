use crate::downloader::chunk_manager::ChunkManger;

pub struct DownloadSingle {}

pub enum DownloadWay {
    Range(ChunkManger),
    Single(DownloadSingle)
}