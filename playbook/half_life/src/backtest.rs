#![allow(unused_imports)]
#![allow(dead_code)]

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
pub struct HalfLifeBacktest {
  pub window: usize,
  pub z_entry: f64,
  pub z_exit: f64,
  pub stop_loss_pct: Option<f64>,
  pub series: RingBuffer<Data>,
  assets: Assets,
}

impl HalfLifeBacktest {
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    capacity: usize,
    window: usize,
    z_entry: f64,
    z_exit: f64,
    ticker: String,
    stop_loss_pct: Option<f64>,
  ) -> Self {
    Self {
      window,
      series: RingBuffer::new(capacity, ticker),
      z_entry,
      z_exit,
      stop_loss_pct,
      assets: Assets::default(),
    }
  }

  pub fn signal(&mut self, ticker: Option<String>) -> anyhow::Result<Vec<Signal>> {
    match ticker {
      None => Ok(vec![]),
      Some(_ticker) => {
        if self.series.vec.len() < self.series.capacity {
          warn!("Insufficient candles to generate signal");
          return Ok(vec![]);
        }
        let s0 = self.series.vec[0].clone();

        let series = Dataset::new(self.series.vec());
        let z = zscore(series.y().as_slice(), self.window)?;

        let mut signals = vec![];

        // --- #1 ---
        let entry_info = SignalInfo {
          price: s0.y,
          date: Time::from_unix_ms(s0.x),
          ticker: self.series.id.clone(),
          quantity: self.assets.cash()?.quantity / s0.y,
        };
        let exit_info = SignalInfo {
          price: s0.y,
          date: Time::from_unix_ms(s0.x),
          ticker: self.series.id.clone(),
          quantity: self.assets.get_or_err(&self.series.id)?.quantity,
        };

        let enter_long = z > self.z_entry;
        let exit_long = z < self.z_exit;
        let exit_short = enter_long;
        let enter_short = exit_long;

        if exit_long {
          signals.push(Signal::ExitLong(exit_info.clone()));
        }
        if exit_short {
          signals.push(Signal::ExitShort(exit_info.clone()));
        }
        if enter_long {
          signals.push(Signal::EnterLong(entry_info.clone()));
        }
        if enter_short {
          signals.push(Signal::EnterShort(entry_info.clone()));
        }

        Ok(signals)
      }
    }
  }
}

impl Strategy<Data> for HalfLifeBacktest {
  /// Appends candle to candle cache and returns a signal (long, short, or do nothing).
  fn process_data(
    &mut self,
    data: Data,
    ticker: Option<String>,
    assets: &Assets,
  ) -> anyhow::Result<Vec<Signal>> {
    if let Some(ticker) = ticker.clone() {
      if ticker == self.series.id {
        self.series.push(Data {
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
      if ticker == self.series.id {
        return Some(&self.series);
      }
    }
    None
  }

  fn stop_loss_pct(&self) -> Option<f64> {
    self.stop_loss_pct
  }

  fn title(&self) -> String {
    "half_life".to_string()
  }
}

// ==========================================================================================
//                                 HalfLife 30m Backtest
// ==========================================================================================

#[tokio::test]
async fn half_life_backtest() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let start_time = Time::new(2022, 7, 1, None, None, None);
  let end_time = Time::new(2024, 7, 1, None, None, None);
  let timeframe = "30m";

  let stop_loss = None;
  let fee = 0.0;
  let slippage = 0.0;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = true;

  let ticker = "BTC".to_string();

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  println!("Backtest {} candles: {}", ticker, series.len());

  let hurst = hurst(series.y().as_slice());
  println!("hurst: {}", hurst);
  let halflife = half_life(&series.y()).unwrap();
  println!("half life: {}", halflife.round());

  // todo: optimize these params
  let z_entry = -2.0;
  let z_exit = 2.0;
  let window = 100; //halflife.round() as usize;

  let strat = HalfLifeBacktest::new(
    window + 1,
    window,
    z_entry,
    z_exit,
    ticker.clone(),
    stop_loss,
  );
  let mut backtest = Backtest::builder(strat)
    .fee(fee)
    .slippage(slippage)
    .bet(bet)
    .leverage(leverage)
    .short_selling(short_selling);
  backtest
    .series
    .insert(ticker.clone(), series.data().clone());

  backtest.execute("HalfLife Backtest", timeframe)?;

  Ok(())
}

#[tokio::test]
async fn optimize_half_life_backtest() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let start_time = Time::new(2022, 7, 1, None, None, None);
  let end_time = Time::new(2024, 7, 1, None, None, None);
  let timeframe = "30m";

  let stop_loss = None;
  let fee = 0.0;
  let slippage = 0.0;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = true;

  let ticker = "BTC".to_string();

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  println!("Backtest {} candles: {}", ticker, series.len());

  let hurst = hurst(series.y().as_slice());
  println!("hurst: {}", hurst);
  let halflife = half_life(&series.y()).unwrap();
  println!("half life: {}", halflife.round());

  struct OptimizeResult {
    pub z_entry: f64,
    pub z_exit: f64,
    pub window: usize,
    pub summary: Summary,
  }

  // optimize these params
  let z_entry_optimize_range = [-3.0, -2.5, -2.0, -1.5, -1.0];
  let z_exit_optimize_range = [1.0, 1.5, 2.0, 2.5, 3.0];
  let window_optimize_range = [2, 100];

  let start = Time::now();
  let summaries: Vec<OptimizeResult> = window_optimize_range
    .into_par_iter()
    .flat_map(|window| {
      let summaries: Vec<OptimizeResult> = z_entry_optimize_range
        .into_par_iter()
        .flat_map(|z_entry| {
          let summaries: Vec<OptimizeResult> = z_exit_optimize_range
            .into_par_iter()
            .flat_map(|z_exit| {
              let strat = HalfLifeBacktest::new(
                window + 1,
                window,
                z_entry,
                z_exit,
                ticker.clone(),
                stop_loss,
              );
              let mut backtest = Backtest::builder(strat)
                .fee(fee)
                .slippage(slippage)
                .bet(bet)
                .leverage(leverage)
                .short_selling(short_selling);
              backtest
                .series
                .insert(ticker.clone(), series.data().clone());
              let summary = backtest.backtest()?;
              Result::<_, anyhow::Error>::Ok(OptimizeResult {
                z_entry,
                z_exit,
                window,
                summary,
              })
            })
            .collect();
          Result::<_, anyhow::Error>::Ok(summaries)
        })
        .flatten()
        .collect();
      Result::<_, anyhow::Error>::Ok(summaries)
    })
    .flatten()
    .collect();

  println!("optimized in {}s", Time::now().to_unix() - start.to_unix());

  let best = summaries
    .iter()
    .max_by(|a, b| {
      a.summary
        .pct_roi(&ticker)
        .partial_cmp(&b.summary.pct_roi(&ticker))
        .unwrap()
    })
    .unwrap();
  println!(
    "optimal z_entry: {}, z_exit: {}, window: {}",
    best.z_entry, best.z_exit, best.window
  );
  best.summary.print(&ticker);

  Ok(())
}
