#![allow(unused_imports)]
#![allow(dead_code)]

use crate::trade::{Bet, Trade};
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
pub struct StatArbBacktest {
  pub window: usize,
  pub zscore_threshold: f64,
  pub stop_loss_pct: Option<f64>,

  pub x: RingBuffer<Data>,
  pub y: RingBuffer<Data>,

  pub x_rebal_pct: f64,
  pub y_rebal_pct: f64,

  assets: Positions,

  pub last_rebal_price: f64,
}

impl StatArbBacktest {
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
      assets: Positions::default(),
      last_rebal_price: 0.0,
    }
  }

  pub fn signal(
    &mut self,
    ticker: Option<String>,
    active_trades: &ActiveTrades,
  ) -> anyhow::Result<Vec<Signal>> {
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
        let x = Dataset::new(self.x.vec()).normalize()?;
        let y = Dataset::new(self.y.vec()).normalize()?;
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

        let mut signals = vec![];

        let id = 0;
        let x_long_key = Trade::build_key(&self.x.id, TradeAction::EnterLong, id);
        let x_short_key = Trade::build_key(&self.x.id, TradeAction::EnterShort, id);
        let y_long_key = Trade::build_key(&self.y.id, TradeAction::EnterLong, id);
        let y_short_key = Trade::build_key(&self.y.id, TradeAction::EnterShort, id);
        let active_x_long = active_trades.get(&x_long_key);
        let active_x_short = active_trades.get(&x_short_key);
        let active_y_long = active_trades.get(&y_long_key);
        let active_y_short = active_trades.get(&y_short_key);

        let mut has_x_long = active_x_long.is_some();
        let mut has_x_short = active_x_short.is_some();
        let mut has_y_long = active_y_long.is_some();
        let mut has_y_short = active_y_short.is_some();

        let x_bet = Bet::Percent(self.x_rebal_pct);
        let y_bet = Bet::Percent(self.y_rebal_pct);

        // --- #1 ---
        let x_enter_long = Signal {
          id,
          price: x_0.y(),
          date: Time::from_unix_ms(x_0.x()),
          ticker: self.x.id.clone(),
          bet: Some(x_bet),
          side: TradeAction::EnterLong,
        };
        let y_enter_long = Signal {
          id,
          price: y_0.y(),
          date: Time::from_unix_ms(y_0.x()),
          ticker: self.y.id.clone(),
          bet: Some(y_bet),
          side: TradeAction::EnterLong,
        };
        let x_exit_long = Signal {
          id,
          price: x_0.y(),
          date: Time::from_unix_ms(x_0.x()),
          ticker: self.x.id.clone(),
          bet: None,
          side: TradeAction::ExitLong,
        };
        let y_exit_long = Signal {
          id,
          price: y_0.y(),
          date: Time::from_unix_ms(y_0.x()),
          ticker: self.y.id.clone(),
          bet: None,
          side: TradeAction::ExitLong,
        };

        let x_enter_short = Signal {
          id,
          price: x_0.y(),
          date: Time::from_unix_ms(x_0.x()),
          ticker: self.x.id.clone(),
          bet: Some(x_bet),
          side: TradeAction::EnterShort,
        };
        let y_enter_short = Signal {
          id,
          price: y_0.y(),
          date: Time::from_unix_ms(y_0.x()),
          ticker: self.y.id.clone(),
          bet: Some(y_bet),
          side: TradeAction::EnterShort,
        };
        let x_exit_short = Signal {
          id,
          price: x_0.y(),
          date: Time::from_unix_ms(x_0.x()),
          ticker: self.x.id.clone(),
          bet: None,
          side: TradeAction::ExitShort,
        };
        let y_exit_short = Signal {
          id,
          price: y_0.y(),
          date: Time::from_unix_ms(y_0.x()),
          ticker: self.y.id.clone(),
          bet: None,
          side: TradeAction::ExitShort,
        };

        let enter_long = z_0.y() > self.zscore_threshold;
        let exit_long = z_0.y() < -self.zscore_threshold;
        let exit_short = exit_long;
        let enter_short = enter_long;

        if exit_long && has_x_long && has_y_long {
          signals.push(x_exit_long.clone());
          signals.push(y_exit_long.clone());
          has_x_long = false;
          has_y_long = false;
        }
        if exit_short && has_x_short && has_y_short {
          signals.push(x_exit_short.clone());
          signals.push(y_exit_short.clone());
          has_x_short = false;
          has_y_short = false;
        }
        if enter_long && !has_x_long && !has_y_long {
          signals.push(x_enter_long.clone());
          signals.push(y_enter_long.clone());
        }
        if enter_short && !has_x_short && !has_y_short {
          signals.push(x_enter_short.clone());
          signals.push(y_enter_short.clone());
        }

        Ok(signals)
      }
    }
  }
}

impl Strategy<Data> for StatArbBacktest {
  /// Appends candle to candle cache and returns a signal (long, short, or do nothing).
  fn process_data(
    &mut self,
    data: Data,
    ticker: Option<String>,
    assets: &Positions,
    active_trades: &ActiveTrades,
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
    self.signal(ticker, active_trades)
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
    "stat_arb".to_string()
  }
}

// ==========================================================================================
//                                 ShannonsStatArb 30m Backtests
// ==========================================================================================

#[tokio::test]
async fn stat_arb_1d_backtest() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2025, 7, 1, None, None, None);
  let timeframe = "1d";

  let fee = 0.0;
  let window = 9;
  let capacity = window + 1;
  let threshold = 2.0;
  let stop_loss = None;
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

  let spread = spread_dynamic(&x_series.y(), &y_series.y()).unwrap();
  let hurst = hurst(spread.as_slice());
  println!("hurst: {}", hurst);
  let halflife = half_life(&spread).unwrap();
  println!("half life: {}", halflife.round());

  let strat = StatArbBacktest::new(
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

  backtest
    .series
    .insert(x_ticker.clone(), x_series.data().clone());
  backtest
    .series
    .insert(y_ticker.clone(), y_series.data().clone());

  backtest.execute("StatArb Backtest", timeframe)?;

  Ok(())
}
