//!
//! # 持久化模块
//! 用于将下载信息持久化到硬盘，用于断点恢复
//!

use std::fs;
use std::path::Path;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use crate::download::download_task::DownloadTaskState;

#[derive(Serialize, Deserialize)]
pub struct PersistenceState {
    pub tasks: Vec<DownloadTaskState>
}

impl PersistenceState {
    pub fn load_from_file(file_path: &str) -> Result<Self> {
        if Path::new(file_path).exists() {
            let data = fs::read_to_string(file_path)?;
            let state: PersistenceState = serde_json::from_str(&data)?;
            
            Ok(state)
        } else {
            Ok(PersistenceState { tasks: Vec::new() })
        }
    }

    pub fn save_to_file(&self, file_path: &str) -> Result<()> {
        let data = serde_json::to_string_pretty(self)?;
        fs::write(file_path, data)?;

        Ok(())
    }
}
