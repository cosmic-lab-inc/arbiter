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

#[tokio::test]
async fn shannons_demon() {
  let tests = 1_000;
  let iterations = 10_000;
  let rebalance_price_delta_threshold = 10.0;
  let initial_net_worth = 400.0;
  let initial_price = 150.0;

  let mut rng = rand::thread_rng();
  let mut final_profits = Vec::new();
  let mut final_prices = Vec::new();

  for _ in 0..tests {
    let mut net_worth = initial_net_worth;
    let mut price = initial_price;
    let mut sol_bal = net_worth / 2.0 / price;
    let mut usdc_bal = net_worth / 2.0;

    for _ in 0..iterations {
      // change price randomly either up or down "rebalance_price_delta_threshold"
      let change: u8 = rng.gen_range(0..2);
      if change == 0 {
        price -= rebalance_price_delta_threshold;
      } else {
        price += rebalance_price_delta_threshold;
      }
      if price <= 0.0 {
        break;
      }

      net_worth = usdc_bal + (sol_bal * price);
      sol_bal = net_worth / 2.0 / price;
      usdc_bal = net_worth / 2.0;
    }
    let profit = (net_worth - initial_net_worth) / initial_net_worth * 100.0;
    final_profits.push(profit);
    final_prices.push(price);
  }
  let avg_final_profit = final_profits.iter().sum::<f64>() / final_profits.len() as f64;
  let avg_final_price = final_prices.iter().sum::<f64>() / final_prices.len() as f64;
  let avg_buy_and_hold_profit = (avg_final_price - initial_price) / initial_price * 100.0;
  println!(
    "Average strategy profit: {}%, Average buy & hold: {}%",
    trunc!(avg_final_profit, 3),
    trunc!(avg_buy_and_hold_profit, 3)
  );
}
