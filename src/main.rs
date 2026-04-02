use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    sidekick::init_tracing();
    sidekick::cli::run().await
}
