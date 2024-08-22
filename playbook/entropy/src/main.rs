use engine::*;
use nexus::drift_client::MarketId;
use nexus::logger::init_logger;

mod backtest;
mod config;
mod engine;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  dotenv::dotenv().ok();
  init_logger();

  let mut client = Engine::new(0, MarketId::SOL_PERP).await?;
  client.start().await?;

  Ok(())
}
