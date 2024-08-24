use engine::*;
use nexus::drift_client::MarketId;
use nexus::*;

mod backtest;
mod config;
mod engine;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  dotenv::dotenv().ok();
  init_logger();

  let market = MarketId::SOL_PERP;
  let mut client = Engine::new(0, market).await?;
  client.start().await?;

  Ok(())
}
