use reqwest::Client;
use anyhow::{Context, Result};

pub async fn get_content_length(client: &Client, url: &str) -> Result<u64> {
    let resp = client.head(url)
        .send()
        .await
        .with_context(|| format!("发送 HEAD 请求失败：{}", &url))?;

    if resp.status().is_success() {
        if let Some(len) = resp.headers().get(reqwest::header::CONTENT_LENGTH) {
            let len = len.to_str()?
                .parse::<u64>()
                .context("解析 Content-Length 失败")?;

            Ok(len)
        } else {
            Err(anyhow::anyhow!("获取 Content-Length 失败"))
        }
    } else {
        Err(anyhow::anyhow!("获取文件大小失败：HTTP {}", resp.status()))
    }
}
