#![allow(warnings)]

use anyhow::Result;
use futures_util::StreamExt;

mod download;
mod task;

#[tokio::main]
async fn main() {
    test_download().await.unwrap();
}

async fn test_download() -> Result<()> {
    Ok(())
}
