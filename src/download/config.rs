//!
//! 配置模块
//!

use std::fs;
use serde::{Deserialize, Serialize};
use anyhow::Result;

#[derive(Deserialize, Serialize, Debug)]
pub struct DownloadConfig {
    pub url: String,
    pub file_name: Option<String>
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Config {
    pub max_concurrent_downloads: usize,
    pub chunk_size: u64,
    pub retry_times: usize,
    pub download_dir: String,
    pub tasks: Vec<DownloadConfig>
}

impl Config {
    pub fn load_from_file(file_path: &str) -> Result<Self> {
        let config_data = fs::read_to_string(file_path)?;
        let config: Config = serde_json::from_str(&config_data)?;

        Ok(config)
    }
}

impl Default for Config {
    fn default() -> Self {
        let dir = dirs::download_dir().unwrap();
        Self {
            max_concurrent_downloads: 6,
            chunk_size: 1024 * 1024 * 10,
            retry_times: 3,
            download_dir: dir.to_str().unwrap().to_string(),
            tasks: Vec::new()
        }
    }
}
