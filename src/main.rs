mod download;
mod cli;
mod api;

#[tokio::main]
async fn main() {
    api::setup_server().await;
}
