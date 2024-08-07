#![allow(unused_imports)]

use crate::math::hurst;
use crate::trade::{Bet, Signal, SignalInfo};
use crate::Backtest;
use crate::Strategy;
use log::warn;
use nexus::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tradestats::kalman::*;
use tradestats::metrics::*;
use tradestats::utils::*;

#[derive(Debug, Clone)]
pub struct DemonBacktest {
  /// Capacity of data cache
  pub capacity: usize,
  /// Last N data from current datum.
  /// 0th index is current datum, Nth index is oldest datum.
  pub series: RingBuffer<Data>,
  pub rebal_price_delta: f64,
  pub equity: f64,
  pub base: f64,
  pub quote: f64,
  pub last_rebal_price: f64,
}

impl DemonBacktest {
  pub fn new(capacity: usize, rebal_price_delta: f64, ticker: String) -> Self {
    Self {
      capacity,
      series: RingBuffer::new(capacity, ticker),
      rebal_price_delta,
      equity: 0.0,
      base: 0.0,
      quote: 0.0,
      last_rebal_price: 0.0,
    }
  }

  pub fn signal(
    &mut self,
    ticker: Option<String>,
    equity: Option<f64>,
  ) -> anyhow::Result<Vec<Signal>> {
    match (ticker, equity) {
      (Some(ticker), Some(equity)) => {
        if ticker != self.series.id {
          Ok(vec![])
        } else {
          if self.series.vec.len() < self.series.capacity {
            warn!("Insufficient candles to generate signal");
            self.equity = equity;

            return Ok(vec![]);
          }
          // todo
          Ok(vec![])
        }
      }
      _ => Ok(vec![]),
    }
  }
}

impl Strategy<Data> for DemonBacktest {
  /// Appends candle to candle cache and returns a signal (long, short, or do nothing).
  fn process_data(
    &mut self,
    data: Data,
    ticker: Option<String>,
    equity: Option<f64>,
  ) -> anyhow::Result<Vec<Signal>> {
    if let Some(ticker) = ticker.clone() {
      if ticker == self.series.id {
        self.series.push(Data {
          x: data.x,
          y: data.y,
        });
      }
    }

    self.signal(ticker, equity)
  }

  fn cache(&self, ticker: Option<String>) -> Option<&RingBuffer<Data>> {
    match ticker {
      None => None,
      Some(ticker) => {
        if ticker == self.series.id {
          Some(&self.series)
        } else {
          None
        }
      }
    }
  }

  fn stop_loss_pct(&self) -> Option<f64> {
    None
  }
}

// ==========================================================================================
//                                 StatArb 30m Backtests
// ==========================================================================================

#[tokio::test]
async fn btc_eth_30m_stat_arb() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let start_time = Time::new(2023, 1, 1, None, None, None);
  let end_time = Time::new(2024, 4, 30, None, None, None);

  let window = 9;
  let capacity = window + 1;
  let threshold = 2.0;
  let stop_loss = Some(5.0);
  let fee = 0.02;
  let slippage = 0.0;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = true;

  let x_ticker = "BTCUSDT".to_string();
  let y_ticker = "ETHUSDT".to_string();

  let btc_path = format!(
    "{}/data/btcusdt_30m.csv",
    workspace_dir().as_os_str().to_str().unwrap()
  );
  println!("btc path: {}", btc_path);
  let btc_csv = PathBuf::from(btc_path);
  let mut x_series =
    Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), x_ticker.clone())?;

  let eth_path = format!(
    "{}/data/ethusdt_30m.csv",
    workspace_dir().as_os_str().to_str().unwrap()
  );
  let eth_csv = PathBuf::from(eth_path);
  let mut y_series =
    Dataset::csv_series(&eth_csv, Some(start_time), Some(end_time), y_ticker.clone())?;

  Dataset::align(&mut x_series, &mut y_series)?;
  assert_eq!(x_series.len(), y_series.len());

  // normalize data using percent change from first price in time series
  let x = Dataset::normalize_series(&x_series)?;
  let y = Dataset::normalize_series(&y_series)?;
  let spread: Vec<f64> = spread_dynamic(&x.y(), &y.y())
    .map_err(|e| anyhow::anyhow!("Error calculating spread: {}", e))?;
  println!(
    "Spread Hurst Exponent: {}",
    trunc!(hurst(spread.clone()), 2)
  );

  let strat = DemonBacktest::new(capacity, threshold, x_ticker.clone());
  let mut backtest = Backtest::new(strat, 1000.0, fee, slippage, bet, leverage, short_selling);
  // Append to backtest data
  backtest
    .series
    .insert(x_ticker.clone(), x_series.data().clone());
  backtest
    .series
    .insert(y_ticker.clone(), y_series.data().clone());
  println!(
    "Backtest BTC candles: {}",
    backtest.series.get(&x_ticker).unwrap().len()
  );
  println!(
    "Backtest ETH candles: {}",
    backtest.series.get(&y_ticker).unwrap().len()
  );

  let summary = backtest.backtest()?;
  let all_buy_and_hold = backtest.buy_and_hold()?;

  if let Some(trades) = backtest.trades.get(&x_ticker) {
    if trades.len() > 1 {
      summary.print(&x_ticker);
      let x_bah = all_buy_and_hold
        .get(&x_ticker)
        .ok_or(anyhow::anyhow!("Buy and hold not found for ticker"))?
        .clone();
      Plot::plot(
        vec![summary.cum_pct(&x_ticker)?.data().clone(), x_bah],
        "stat_arb_btc_30m_backtest.png",
        &format!("{} Stat Arb Backtest", x_ticker),
        "% ROI",
        "Unix Millis",
      )?;
    }
  }
  if let Some(trades) = backtest.trades.get(&y_ticker) {
    if trades.len() > 1 {
      summary.print(&y_ticker);
      let y_bah = all_buy_and_hold
        .get(&y_ticker)
        .ok_or(anyhow::anyhow!("Buy and hold not found for ticker"))?
        .clone();
      Plot::plot(
        vec![summary.cum_pct(&y_ticker)?.data().clone(), y_bah],
        "stat_arb_eth_30m_backtest.png",
        &format!("{} Stat Arb Backtest", y_ticker),
        "% ROI",
        "Unix Millis",
      )?;
    }
  }

  Ok(())
}
