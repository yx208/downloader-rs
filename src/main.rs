#![allow(warnings)]

use std::num::{NonZeroU8, NonZeroUsize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use url::Url;
use anyhow::Result;
use uuid::Uuid;
use crate::download::downloader::{Downloader, DownloaderConfig};
use crate::download::error::DownloadEndCause;
use crate::task::task::DownloadTask;

mod download;
mod task;

#[tokio::main]
async fn main() {
    test_download().await.unwrap();
}

async fn test_download() -> Result<()> {
    let config = DownloaderConfig::from_url(
        Url::parse("https://tasset.xgy.tv/down/resources/agency/CeShiJiGou_1/QianDuanBuMen_2/dc9152119c160a601e5f684795fb4ea2_16/20241118/18154964fcaafce36522519013.mp4")?
    );

    let mut task = DownloadTask::new(Uuid::new_v4(), config);
    let future = task.download().await?;
    future.await?;

    Ok(())

    // let downloader = Arc::new(Mutex::new(Downloader::new()));
    // let run_download = || async {
    //     let mut guard = downloader.lock().await;
    //     guard.download(config.clone(), None).await.unwrap()
    // };
    //
    // // 等一秒暂停
    // let downloader_clone = downloader.clone();
    // tokio::spawn(async move {
    //     tokio::time::sleep(Duration::from_millis(500)).await;
    //     let mut guard = downloader_clone.lock().await;
    //     guard.pause();
    // });
    //
    // // 运行
    // let future = run_download().await;
    // assert_eq!(future.await?, DownloadEndCause::Paused);
    // println!("暂停");
    //
    // tokio::time::sleep(Duration::from_secs(2)).await;
    //
    // // 经过暂停后再次运行
    // let future = run_download().await;
    // match future.await {
    //     Ok(result) => println!("OK: {:?}", result),
    //     Err(err) => println!("ERR: {err}")
    // }
    //
    // // 计算下载是否完整
    // let (nature_length, disk_length) = {
    //     let guard= downloader.lock().await;
    //     let disk_size = config.file_path().metadata()?.len();
    //     (guard.content_length.unwrap(), disk_size)
    // };
    // assert_eq!(nature_length, disk_length);
    //
    // Ok(())
}

async fn test_progress_bar() {
    let mut progress_bar = task::progress_bar::ProgressBar::new(256);

    loop {
        progress_bar.print(
            (1024 * 1024 * 6),
            (1024 * 1024 * 20),
            (1024 * 1024),
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
