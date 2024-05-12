use solana_sdk::pubkey::Pubkey;

use common::{Data, trunc};

#[derive(Debug)]
pub struct Trader {
  pub key: Pubkey,
  pub data: Vec<Data>,
}

impl Trader {
  /// Average USDC PnL per trade
  pub fn avg_dollar_return(&self) -> f64 {
    let avg_usdc_pnl = self
      .data
      .windows(2)
      .map(|pair| {
        let (a, b) = (pair[1].y, pair[0].y);
        b - a
      })
      .sum::<f64>()
      / self.data.len() as f64;
    trunc!(avg_usdc_pnl, 2)
  }

  /// Average percent PnL per trade
  pub fn avg_trade(&self) -> f64 {
    let avg = self
      .data
      .windows(2)
      .map(|pair| {
        let (a, b) = (pair[1].y, pair[0].y);
        (b - a) / a * 100.0
      })
      .sum::<f64>()
      / self.data.len() as f64;
    trunc!(avg, 2)
  }

  /// Worst percent PnL on a single trade
  pub fn worst_trade(&self) -> f64 {
    // compute percent difference between each data point
    let trade_pnls = self.data.windows(2).map(|pair| {
      let (a, b) = (pair[0].y, pair[1].y);
      trunc!((b - a) / a * 100.0, 2)
    });
    // find the worst trade
    trade_pnls.fold(f64::MAX, |a, b| a.min(b))
  }

  pub fn total_dollar_pnl(&self) -> f64 {
    let pnl = self
      .data
      .windows(2)
      .map(|pair| {
        let (a, b) = (pair[0].y, pair[1].y);
        b - a
      })
      .sum::<f64>();
    trunc!(pnl, 2)
  }

  pub fn total_pct_pnl(&self) -> f64 {
    let pnl = self
      .data
      .windows(2)
      .map(|pair| {
        let (a, b) = (pair[0].y, pair[1].y);
        trunc!((b - a) / a * 100.0, 2)
      })
      .sum::<f64>();
    trunc!(pnl, 2)
  }

  pub fn start_slot(&self) -> i64 {
    self.data.last().unwrap().x
  }

  pub fn end_slot(&self) -> i64 {
    self.data.first().unwrap().x
  }
}
