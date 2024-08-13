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
  pub window: usize,
  pub zscore_threshold: f64,
  pub stop_loss_pct: Option<f64>,

  pub x: RingBuffer<Data>,
  pub y: RingBuffer<Data>,

  pub x_rebal_pct: f64,
  pub y_rebal_pct: f64,

  assets: Assets,

  pub last_rebal_price: f64,
}

impl DemonBacktest {
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    capacity: usize,
    window: usize,
    zscore_threshold: f64,
    x_ticker: String,
    y_ticker: String,
    stop_loss_pct: Option<f64>,
    x_rebal_pct: f64,
    y_rebal_pct: f64,
  ) -> Self {
    Self {
      window,
      x: RingBuffer::new(capacity, x_ticker),
      y: RingBuffer::new(capacity, y_ticker),
      zscore_threshold,
      stop_loss_pct,
      x_rebal_pct,
      y_rebal_pct,
      assets: Assets::new(),
      last_rebal_price: 0.0,
    }
  }

  pub fn signal(&mut self, ticker: Option<String>) -> anyhow::Result<Vec<Signal>> {
    match ticker {
      None => Ok(vec![]),
      Some(_ticker) => {
        if self.x.vec.len() < self.x.capacity || self.y.vec.len() < self.y.capacity {
          warn!("Insufficient candles to generate signal");
          return Ok(vec![]);
        }
        let x_0 = self.x.vec[0].clone();
        let y_0 = self.y.vec[0].clone();

        // todo: will this work live?
        if x_0.x() != y_0.x() {
          return Ok(vec![]);
        }

        // compare spread
        // let x = Dataset::new(self.x.vec()).normalize_series()?;
        // let y = Dataset::new(self.y.vec()).normalize_series()?;
        let x = Dataset::new(self.x.vec());
        let y = Dataset::new(self.y.vec());
        assert_eq!(x.len(), self.x.len());
        assert_eq!(y.len(), self.y.len());

        let spread: Vec<f64> = spread_dynamic(&x.y(), &y.y())
          .map_err(|e| anyhow::anyhow!("Error calculating spread: {}", e))?;
        assert_eq!(spread.len(), y.len());
        assert_eq!(spread.len(), x.len());

        let lag_spread = spread[..spread.len() - 1].to_vec();
        let spread = spread[1..].to_vec();

        assert_eq!(spread.len(), lag_spread.len());
        assert_eq!(lag_spread.len(), self.window);
        assert_eq!(spread.len(), self.window);

        let z_0 = Data {
          x: x_0.x(),
          y: zscore(&spread, self.window)?,
        };

        let x_enter_info = SignalInfo {
          price: x_0.y(),
          date: Time::from_unix_ms(x_0.x()),
          ticker: self.x.id.clone(),
          quantity: self.assets.cash() / x_0.y(),
        };
        let y_enter_info = SignalInfo {
          price: y_0.y(),
          date: Time::from_unix_ms(y_0.x()),
          ticker: self.y.id.clone(),
          quantity: self.assets.cash() / y_0.y(),
        };
        let x_exit_info = SignalInfo {
          price: x_0.y(),
          date: Time::from_unix_ms(x_0.x()),
          ticker: self.x.id.clone(),
          quantity: *self.assets.get(&self.x.id).unwrap_or(&0.0),
        };
        let y_exit_info = SignalInfo {
          price: y_0.y(),
          date: Time::from_unix_ms(y_0.x()),
          ticker: self.y.id.clone(),
          quantity: *self.assets.get(&self.y.id).unwrap_or(&0.0),
        };

        let mut signals = vec![];

        // --- #1 ---
        let enter_long = z_0.y() > self.zscore_threshold;
        let exit_long = z_0.y() < -self.zscore_threshold;
        let exit_short = exit_long;
        let enter_short = enter_long;

        if exit_long {
          signals.push(Signal::ExitLong(x_exit_info.clone()));
          signals.push(Signal::ExitLong(y_exit_info.clone()));
        }
        if exit_short {
          signals.push(Signal::ExitShort(x_exit_info.clone()));
          signals.push(Signal::ExitShort(y_exit_info.clone()));
        }
        if enter_long {
          signals.push(Signal::EnterLong(x_enter_info.clone()));
          signals.push(Signal::EnterLong(y_enter_info.clone()));
        }
        if enter_short {
          signals.push(Signal::EnterShort(x_enter_info.clone()));
          signals.push(Signal::EnterShort(y_enter_info.clone()));
        }

        Ok(signals)
      }
    }
  }
}

impl Strategy<Data> for DemonBacktest {
  /// Appends candle to candle cache and returns a signal (long, short, or do nothing).
  fn process_data(
    &mut self,
    data: Data,
    ticker: Option<String>,
    assets: &Assets,
  ) -> anyhow::Result<Vec<Signal>> {
    if let Some(ticker) = ticker.clone() {
      if ticker == self.x.id {
        self.x.push(Data {
          x: data.x,
          y: data.y,
        });
      } else if ticker == self.y.id {
        self.y.push(Data {
          x: data.x,
          y: data.y,
        });
      }
    }
    self.assets = assets.clone();
    self.signal(ticker)
  }

  fn cache(&self, ticker: Option<String>) -> Option<&RingBuffer<Data>> {
    if let Some(ticker) = ticker {
      if ticker == self.x.id {
        Some(&self.x)
      } else if ticker == self.y.id {
        Some(&self.y)
      } else {
        None
      }
    } else {
      None
    }
  }

  fn stop_loss_pct(&self) -> Option<f64> {
    self.stop_loss_pct
  }

  fn title(&self) -> String {
    "demon".to_string()
  }
}

// ==========================================================================================
//                                 ShannonsDemon 30m Backtests
// ==========================================================================================

#[tokio::test]
async fn btc_eth_30m_demon() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let start_time = Time::new(2020, 7, 1, None, None, None);
  let end_time = Time::new(2024, 7, 1, None, None, None);
  let timeframe = "30m";

  let window = 9;
  let capacity = window + 1;
  let threshold = 2.0;
  let stop_loss = None;
  let fee = 0.0;
  let slippage = 0.0;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = false;

  let x_ticker = "BTC".to_string();
  let y_ticker = "ETH".to_string();

  let x_rebal_pct = 50.0;
  let y_rebal_pct = 50.0;

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let mut x_series =
    Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), x_ticker.clone())?;

  let eth_csv = workspace_path(&format!("data/eth_{}.csv", timeframe));
  let mut y_series =
    Dataset::csv_series(&eth_csv, Some(start_time), Some(end_time), y_ticker.clone())?;

  Dataset::align(&mut x_series, &mut y_series)?;
  assert_eq!(x_series.len(), y_series.len());

  // normalize data using percent change from first price in time series
  let x = Dataset::normalize_series(&x_series)?;
  let y = Dataset::normalize_series(&y_series)?;
  let spread: Vec<f64> = spread_dynamic(&x.y(), &y.y())
    .map_err(|e| anyhow::anyhow!("Error calculating spread: {}", e))?;

  let strat = DemonBacktest::new(
    capacity,
    window,
    threshold,
    x_ticker.clone(),
    y_ticker.clone(),
    stop_loss,
    x_rebal_pct,
    y_rebal_pct,
  );
  let mut backtest = Backtest::builder(strat)
    .fee(fee)
    .slippage(slippage)
    .bet(bet)
    .leverage(leverage)
    .short_selling(short_selling);
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

  backtest.execute("Demon Backtest", timeframe)?;

  Ok(())
}
