#![allow(warnings)]

use std::time::Duration;

mod download;
mod task;

#[tokio::main]
async fn main() {
    test_progress_bar().await;
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
