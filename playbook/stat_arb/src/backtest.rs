#![allow(unused_imports)]

use crate::math::hurst;
use crate::trade::{Bet, Signal, SignalInfo};
use crate::{Backtest, Dataset, Strategy};
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
  /// Capacity of data caches
  pub capacity: usize,
  /// Window to compute zscores
  pub window: usize,
  /// Last N data from current datum.
  /// 0th index is current datum, Nth index is oldest datum.
  pub x: RingBuffer<Data>,
  /// Last N data from current datum.
  /// 0th index is current datum, Nth index is oldest datum.
  pub y: RingBuffer<Data>,
  pub zscore_threshold: f64,
  pub stop_loss_pct: Option<f64>,
}

impl StatArbBacktest {
  pub fn new(
    capacity: usize,
    window: usize,
    zscore_threshold: f64,
    x_ticker: String,
    y_ticker: String,
    stop_loss_pct: Option<f64>,
  ) -> Self {
    Self {
      capacity,
      window,
      x: RingBuffer::new(capacity, x_ticker),
      y: RingBuffer::new(capacity, y_ticker),
      zscore_threshold,
      stop_loss_pct,
    }
  }

  /// ZScore of last index in a spread time series
  pub fn zscore(series: &[f64], window: usize) -> anyhow::Result<f64> {
    // Guard: Ensure correct window size
    if window > series.len() {
      return Err(anyhow::anyhow!("Window size is greater than vector length"));
    }

    // last z score
    let window_data: &[f64] = &series[series.len() - window..];
    let mean: f64 = window_data.iter().sum::<f64>() / window_data.len() as f64;
    let var: f64 = window_data
      .iter()
      .map(|&val| (val - mean).powi(2))
      .sum::<f64>()
      / (window_data.len() - 1) as f64;
    let std_dev: f64 = var.sqrt();
    if std_dev == 0.0 {
      warn!(
        "Standard deviation is zero with var {}, mean {}, and len {}",
        var,
        mean,
        window_data.len()
      );
      return Ok(0.0);
    }
    let z_score = (series[series.len() - 1] - mean) / std_dev;
    Ok(z_score)
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
          y: Self::zscore(&spread, self.window)?,
        };

        let x_info = SignalInfo {
          price: x_0.y(),
          date: Time::from_unix_ms(x_0.x()),
          ticker: self.x.id.clone(),
        };
        let y_info = SignalInfo {
          price: y_0.y(),
          date: Time::from_unix_ms(y_0.x()),
          ticker: self.y.id.clone(),
        };

        let mut signals = vec![];

        // --- #1 ---
        let enter_long = z_0.y() > self.zscore_threshold;
        let exit_long = z_0.y() < -self.zscore_threshold;
        let exit_short = exit_long;
        let enter_short = enter_long;

        if exit_long {
          signals.push(Signal::ExitLong(x_info.clone()));
          signals.push(Signal::ExitLong(y_info.clone()));
        }
        if exit_short {
          signals.push(Signal::ExitShort(x_info.clone()));
          signals.push(Signal::ExitShort(y_info.clone()));
        }
        if enter_long {
          signals.push(Signal::EnterLong(x_info.clone()));
          signals.push(Signal::EnterLong(y_info.clone()));
        }
        if enter_short {
          signals.push(Signal::EnterShort(x_info.clone()));
          signals.push(Signal::EnterShort(y_info.clone()));
        }

        // --- #2 ---
        // when x enters long/short, y exits short/long
        // let enter_long_x = z_0.y() > self.zscore_threshold;
        // let exit_short_x = enter_long_x;
        // let enter_short_y = enter_long_x;
        // let exit_long_y = enter_long_x;
        //
        // let enter_long_y = z_0.y() < -self.zscore_threshold;
        // let exit_short_y = enter_long_y;
        // let enter_short_x = enter_long_y;
        // let exit_long_x = enter_long_y;

        // --- #3 ---
        // let enter_long_x = z_0.y() > self.zscore_threshold;
        // let enter_short_y = enter_long_x;
        //
        // let enter_long_y = z_0.y() < -self.zscore_threshold;
        // let enter_short_x = enter_long_y;
        //
        // let exit_long_x = z_0.y().abs() < 0.5;
        // let exit_long_y = z_0.y().abs() < 0.5;
        // let exit_short_x = z_0.y().abs() < 0.5;
        // let exit_short_y = z_0.y().abs() < 0.5;
        //
        // if exit_long_x {
        //   signals.push(Signal::ExitLong(x_info.clone()));
        // }
        // if exit_long_y {
        //   signals.push(Signal::ExitLong(y_info.clone()));
        // }
        // if exit_short_x {
        //   signals.push(Signal::ExitShort(x_info.clone()));
        // }
        // if exit_short_y {
        //   signals.push(Signal::ExitShort(y_info.clone()));
        // }
        //
        // if enter_long_x {
        //   signals.push(Signal::EnterLong(x_info.clone()));
        // }
        // if enter_short_y {
        //   signals.push(Signal::EnterShort(y_info.clone()));
        // }
        //
        // if enter_long_y {
        //   signals.push(Signal::EnterLong(y_info.clone()));
        // }
        // if enter_short_x {
        //   signals.push(Signal::EnterShort(x_info.clone()));
        // }

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
    _equity: Option<f64>,
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
}

// ==========================================================================================
//                                 Misc Tests
// ==========================================================================================

#[tokio::test]
async fn date_ranges() -> anyhow::Result<()> {
  let btc_ticker = "BTC".to_string();
  let btc_csv = workspace_path("data/btc_1m.csv");
  let btc_series = Dataset::csv_series(&btc_csv, None, None, btc_ticker.clone())?;

  let btc_start_ts = btc_series.0.first().unwrap().x();
  let btc_start = Time::from_unix_ms(btc_start_ts);
  let btc_end_ts = btc_series.0.last().unwrap().x();
  let btc_end = Time::from_unix_ms(btc_end_ts);
  println!(
    "BTC date range: {} - {}",
    btc_start.to_string(),
    btc_end.to_string()
  );

  let eth_ticker = "ETH".to_string();
  let eth_csv = workspace_path("data/eth_1m.csv");
  let eth_series = Dataset::csv_series(&eth_csv, None, None, eth_ticker.clone())?;

  let eth_start_ts = eth_series.0.first().unwrap().x();
  let eth_start = Time::from_unix_ms(eth_start_ts);
  let eth_end_ts = eth_series.0.last().unwrap().x();
  let eth_end = Time::from_unix_ms(eth_end_ts);
  println!(
    "ETH date range: {} - {}",
    eth_start.to_string(),
    eth_end.to_string()
  );

  let sol_ticker = "SOL".to_string();
  let sol_csv = workspace_path("data/sol_1m.csv");
  let sol_series = Dataset::csv_series(&sol_csv, None, None, sol_ticker.clone())?;

  let sol_start_ts = *sol_series.x().first().unwrap();
  let sol_start = Time::from_unix_ms(sol_start_ts);
  let sol_end_ts = *sol_series.x().last().unwrap();
  let sol_end = Time::from_unix_ms(sol_end_ts);
  println!(
    "SOL date range: {} - {}",
    sol_start.to_string(),
    sol_end.to_string()
  );

  Ok(())
}

#[test]
fn build_1d_csv() -> anyhow::Result<()> {
  let btc_ticker = "BTC".to_string();
  let btc_csv = workspace_path("data/btc_1m.csv");
  let btc_series = Dataset::csv_series(&btc_csv, None, None, btc_ticker)?;
  let btc_daily_bars: Vec<Bar> = btc_series
    .0
    .iter()
    .step_by(60 * 24)
    .map(|data| Bar {
      date: Time::from_unix_ms(data.x),
      open: data.y,
      high: data.y,
      low: data.y,
      close: data.y,
      volume: None,
    })
    .collect();
  let btc_out_file = workspace_path("data/btc_1d.csv");
  Dataset::write_csv(btc_daily_bars, btc_out_file)?;

  let eth_ticker = "ETH".to_string();
  let eth_csv = workspace_path("data/eth_1m.csv");
  let eth_series = Dataset::csv_series(&eth_csv, None, None, eth_ticker)?;
  let eth_daily_bars: Vec<Bar> = eth_series
    .0
    .iter()
    .step_by(60 * 24)
    .map(|data| Bar {
      date: Time::from_unix_ms(data.x),
      open: data.y,
      high: data.y,
      low: data.y,
      close: data.y,
      volume: None,
    })
    .collect();
  let eth_out_file = workspace_path("data/eth_1d.csv");
  Dataset::write_csv(eth_daily_bars, eth_out_file)?;

  let sol_ticker = "SOL".to_string();
  let sol_csv = workspace_path("data/sol_1m.csv");
  let sol_series = Dataset::csv_series(&sol_csv, None, None, sol_ticker)?;
  let sol_daily_bars: Vec<Bar> = sol_series
    .0
    .iter()
    .step_by(60 * 24)
    .map(|data| Bar {
      date: Time::from_unix_ms(data.x),
      open: data.y,
      high: data.y,
      low: data.y,
      close: data.y,
      volume: None,
    })
    .collect();
  let sol_out_file = workspace_path("data/sol_1d.csv");
  Dataset::write_csv(sol_daily_bars, sol_out_file)?;

  Ok(())
}

#[test]
fn build_1h_csv() -> anyhow::Result<()> {
  let btc_ticker = "BTC".to_string();
  let btc_csv = workspace_path("data/btc_1m.csv");
  let btc_series = Dataset::csv_series(&btc_csv, None, None, btc_ticker)?;
  let btc_daily_bars: Vec<Bar> = btc_series
    .0
    .iter()
    .step_by(60)
    .map(|data| Bar {
      date: Time::from_unix_ms(data.x),
      open: data.y,
      high: data.y,
      low: data.y,
      close: data.y,
      volume: None,
    })
    .collect();
  let btc_out_file = workspace_path("data/btc_1h.csv");
  Dataset::write_csv(btc_daily_bars, btc_out_file)?;

  let eth_ticker = "ETH".to_string();
  let eth_csv = workspace_path("data/eth_1m.csv");
  let eth_series = Dataset::csv_series(&eth_csv, None, None, eth_ticker)?;
  let eth_daily_bars: Vec<Bar> = eth_series
    .0
    .iter()
    .step_by(60)
    .map(|data| Bar {
      date: Time::from_unix_ms(data.x),
      open: data.y,
      high: data.y,
      low: data.y,
      close: data.y,
      volume: None,
    })
    .collect();
  let eth_out_file = workspace_path("data/eth_1h.csv");
  Dataset::write_csv(eth_daily_bars, eth_out_file)?;

  let sol_ticker = "SOL".to_string();
  let sol_csv = workspace_path("data/sol_1m.csv");
  let sol_series = Dataset::csv_series(&sol_csv, None, None, sol_ticker)?;
  let sol_daily_bars: Vec<Bar> = sol_series
    .0
    .iter()
    .step_by(60)
    .map(|data| Bar {
      date: Time::from_unix_ms(data.x),
      open: data.y,
      high: data.y,
      low: data.y,
      close: data.y,
      volume: None,
    })
    .collect();
  let sol_out_file = workspace_path("data/sol_1h.csv");
  Dataset::write_csv(sol_daily_bars, sol_out_file)?;

  Ok(())
}

// ==========================================================================================
//                                 1m Backtests
// ==========================================================================================

#[tokio::test]
async fn btc_eth_1m_stat_arb() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let start_time = Time::new(2017, 12, 1, None, None, None);
  let end_time = Time::new(2018, 2, 1, None, None, None);

  let window = 9;
  let capacity = window + 1;
  let threshold = 2.0;
  let stop_loss = None; //Some(5.0);
  let fee = 0.25;
  let slippage = 0.01;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = true;

  let x_ticker = "BTCUSDT".to_string();
  let y_ticker = "ETHUSDT".to_string();

  let btc_csv = workspace_path("data/btc_1m.csv");
  let mut x_series =
    Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), x_ticker.clone())?;

  let eth_csv = workspace_path("data/eth_1m.csv");
  let mut y_series =
    Dataset::csv_series(&eth_csv, Some(start_time), Some(end_time), y_ticker.clone())?;

  Dataset::align(&mut x_series, &mut y_series)?;
  assert_eq!(x_series.len(), y_series.len());

  let strat = StatArbBacktest::new(
    capacity,
    window,
    threshold,
    x_ticker.clone(),
    y_ticker.clone(),
    stop_loss,
  );
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
    backtest.get_series(&x_ticker)?.len()
  );
  println!(
    "Backtest ETH candles: {}",
    backtest.get_series(&y_ticker)?.len()
  );

  let now = Time::now();
  let summary = backtest.backtest()?;
  let all_buy_and_hold = backtest.buy_and_hold()?;
  println!(
    "backtest finished in {}s",
    Time::now().to_unix() - now.to_unix()
  );

  if let Ok(trades) = backtest.get_trades(&x_ticker) {
    if trades.len() > 1 {
      summary.print(&x_ticker);
      let x_bah = all_buy_and_hold
        .get(&x_ticker)
        .ok_or(anyhow::anyhow!("Buy and hold not found for ticker"))?
        .clone();
      Plot::plot(
        vec![summary.cum_pct(&x_ticker)?.data().clone(), x_bah],
        "btc_1m_backtest.png",
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
        "eth_1m_backtest.png",
        &format!("{} Stat Arb Backtest", y_ticker),
        "% ROI",
        "Unix Millis",
      )?;
    }
  }

  Ok(())
}

#[tokio::test]
async fn btc_eth_1m_cointegration() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let start_time = Time::new(2017, 12, 1, None, None, None);
  let end_time = Time::new(2018, 2, 1, None, None, None);

  let x_ticker = "BTCUSDT".to_string();
  let y_ticker = "ETHUSDT".to_string();

  let mut backtest = Backtest::default();
  let btc_csv = workspace_path("data/btc_1m.csv");
  let mut x_series =
    Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), x_ticker.clone())?;
  let eth_csv = workspace_path("data/eth_1m.csv");
  let mut y_series =
    Dataset::csv_series(&eth_csv, Some(start_time), Some(end_time), y_ticker.clone())?;

  Dataset::align(&mut x_series, &mut y_series)?;
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

  // normalize data using percent change from first price in time series
  let x = Dataset::new(backtest.get_series(&x_ticker)?.clone());
  let y = Dataset::new(backtest.get_series(&y_ticker)?.clone());
  assert_eq!(x.len(), y.len());

  let coint = engle_granger_cointegration_test(&x.y(), &y.y())
    .map_err(|e| anyhow::anyhow!("Error calculating Engle-Granger cointegration test: {}", e))?;
  println!("Cointegration Test: {:#?}", coint);

  let spread: Vec<f64> = spread_standard(&x.y(), &y.y())
    .map_err(|e| anyhow::anyhow!("Error calculating dynamic spread: {}", e))?;
  assert_eq!(spread.len(), y.len());
  assert_eq!(spread.len(), x.len());

  let half_life = half_life(&spread).unwrap().abs().round() as usize;
  println!("Spread half life: {}", half_life);
  let window = 100;

  let roll_coint = Dataset::new(
    rolling_cointegration(&x.y(), &y.y(), window)
      .map_err(|e| anyhow::anyhow!("Error calculating rolling cointegration: {}", e))?
      .iter()
      .enumerate()
      .map(|(i, x)| Data { x: i as i64, y: *x })
      .collect(),
  );
  Plot::plot(
    vec![roll_coint.0],
    "btc_eth_1m_coint.png",
    "BTC/ETH Cointegration",
    "Cointegration",
    "Time",
  )?;

  let roll_corr = rolling_correlation(&x.y(), &y.y(), window)
    .map_err(|e| anyhow::anyhow!("Error calculating rolling correlation: {}", e))?;
  assert_eq!(roll_corr.len(), y.len());
  assert_eq!(roll_corr.len(), x.len());
  let correlation = Dataset::new(
    roll_corr
      .iter()
      .enumerate()
      .map(|(i, x)| Data { x: i as i64, y: *x })
      .collect(),
  );
  Plot::plot(
    vec![correlation.0],
    "btc_eth_1m_corr.png",
    "BTC/ETH Correlation",
    "Correlation",
    "Time",
  )?;

  let spread_dataset = Dataset::new(
    spread
      .iter()
      .enumerate()
      .map(|(i, x)| Data { x: i as i64, y: *x })
      .collect(),
  );
  Plot::plot(
    vec![spread_dataset.0],
    "btc_eth_1m_spread.png",
    "BTC/ETH Spread",
    "Spread",
    "Time",
  )?;

  let zscore: Vec<f64> = rolling_zscore(&spread, window).unwrap();
  assert_eq!(zscore.len(), spread.len());
  let zscore = Dataset::new(
    zscore
      .iter()
      .enumerate()
      .map(|(i, x)| Data { x: i as i64, y: *x })
      .collect(),
  );
  Plot::plot(
    vec![zscore.0],
    "btc_eth_1m_spread_zscore.png",
    "BTC/ETH Spread Z-Score",
    "Z-Score",
    "Time",
  )?;

  Ok(())
}

// ==========================================================================================
//                                 30m Backtests
// ==========================================================================================

#[tokio::test]
async fn btc_eth_30m_stat_arb() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let window = 9;
  let capacity = window + 1;
  let threshold = 2.0;
  let stop_loss = None; //Some(5.0);
  let fee = 0.025;
  let slippage = 0.01;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = false;

  let x_ticker = "BTCUSDT".to_string();
  let y_ticker = "ETHUSDT".to_string();

  let btc_csv = workspace_path("data/btc_30m.csv");
  let mut x_series = Dataset::csv_series(&btc_csv, None, None, x_ticker.clone())?;

  let eth_csv = workspace_path("data/eth_30m.csv");
  let mut y_series = Dataset::csv_series(&eth_csv, None, None, y_ticker.clone())?;

  Dataset::align(&mut x_series, &mut y_series)?;
  assert_eq!(x_series.len(), y_series.len());

  let strat = StatArbBacktest::new(
    capacity,
    window,
    threshold,
    x_ticker.clone(),
    y_ticker.clone(),
    stop_loss,
  );
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
    backtest.get_series(&x_ticker)?.len()
  );
  println!(
    "Backtest ETH candles: {}",
    backtest.get_series(&y_ticker)?.len()
  );

  let now = Time::now();
  let summary = backtest.backtest()?;
  let all_buy_and_hold = backtest.buy_and_hold()?;
  println!(
    "backtest finished in {}s",
    Time::now().to_unix() - now.to_unix()
  );

  if let Ok(trades) = backtest.get_trades(&x_ticker) {
    if trades.len() > 1 {
      summary.print(&x_ticker);
      let x_bah = all_buy_and_hold
        .get(&x_ticker)
        .ok_or(anyhow::anyhow!("Buy and hold not found for ticker"))?
        .clone();
      Plot::plot(
        vec![summary.cum_pct(&x_ticker)?.data().clone(), x_bah],
        "btc_30m_backtest.png",
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
        "eth_30m_backtest.png",
        &format!("{} Stat Arb Backtest", y_ticker),
        "% ROI",
        "Unix Millis",
      )?;
    }
  }

  Ok(())
}

#[tokio::test]
async fn btc_eth_30m_cointegration() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let x_ticker = "BTCUSDT".to_string();
  let y_ticker = "ETHUSDT".to_string();

  let mut backtest = Backtest::default();
  let btc_csv = workspace_path("data/btc_30m.csv");
  let mut x_series = Dataset::csv_series(&btc_csv, None, None, x_ticker.clone())?;
  let eth_csv = workspace_path("data/eth_30m.csv");
  let mut y_series = Dataset::csv_series(&eth_csv, None, None, y_ticker.clone())?;

  Dataset::align(&mut x_series, &mut y_series)?;
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

  // normalize data using percent change from first price in time series
  // let x = Dataset::new(backtest.get_series(&x_ticker)?.clone()).normalize_series()?;
  // let y = Dataset::new(backtest.get_series(&y_ticker)?.clone()).normalize_series()?;
  let x = Dataset::new(backtest.get_series(&x_ticker)?.clone());
  let y = Dataset::new(backtest.get_series(&y_ticker)?.clone());
  assert_eq!(x.len(), y.len());

  let coint = engle_granger_cointegration_test(&x.y(), &y.y())
    .map_err(|e| anyhow::anyhow!("Error calculating Engle-Granger cointegration test: {}", e))?;
  println!("Cointegration Test: {:#?}", coint);

  let spread: Vec<f64> = spread_standard(&x.y(), &y.y())
    .map_err(|e| anyhow::anyhow!("Error calculating dynamic spread: {}", e))?;
  assert_eq!(spread.len(), y.len());
  assert_eq!(spread.len(), x.len());

  let half_life = half_life(&spread).unwrap().abs().round() as usize;
  println!("Spread half life: {}", half_life);
  let window = 100;

  let roll_coint = Dataset::new(
    rolling_cointegration(&x.y(), &y.y(), window)
      .map_err(|e| anyhow::anyhow!("Error calculating rolling cointegration: {}", e))?
      .iter()
      .enumerate()
      .map(|(i, x)| Data { x: i as i64, y: *x })
      .collect(),
  );
  Plot::plot(
    vec![roll_coint.0],
    "btc_eth_30m_coint.png",
    "BTC/ETH Cointegration",
    "Cointegration",
    "Time",
  )?;

  let roll_corr = rolling_correlation(&x.y(), &y.y(), window)
    .map_err(|e| anyhow::anyhow!("Error calculating rolling correlation: {}", e))?;
  assert_eq!(roll_corr.len(), y.len());
  assert_eq!(roll_corr.len(), x.len());
  let correlation = Dataset::new(
    roll_corr
      .iter()
      .enumerate()
      .map(|(i, x)| Data { x: i as i64, y: *x })
      .collect(),
  );
  Plot::plot(
    vec![correlation.0],
    "btc_eth_30m_corr.png",
    "BTC/ETH Correlation",
    "Correlation",
    "Time",
  )?;

  let spread_dataset = Dataset::new(
    spread
      .iter()
      .enumerate()
      .map(|(i, x)| Data { x: i as i64, y: *x })
      .collect(),
  );
  Plot::plot(
    vec![spread_dataset.0],
    "btc_eth_30m_spread.png",
    "BTC/ETH Spread",
    "Spread",
    "Time",
  )?;

  let zscore: Vec<f64> = rolling_zscore(&spread, window).unwrap();
  assert_eq!(zscore.len(), spread.len());
  let zscore = Dataset::new(
    zscore
      .iter()
      .enumerate()
      .map(|(i, x)| Data { x: i as i64, y: *x })
      .collect(),
  );
  Plot::plot(
    vec![zscore.0],
    "btc_eth_30m_spread_zscore.png",
    "BTC/ETH Spread Z-Score",
    "Z-Score",
    "Time",
  )?;

  Ok(())
}

#[tokio::test]
async fn btc_eth_30m_spread_attributes() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let start_time = Time::new(2024, 4, 29, None, None, None);
  let end_time = Time::new(2024, 4, 30, None, None, None);

  let x_ticker = "BTCUSDT".to_string();
  let y_ticker = "ETHUSDT".to_string();

  let btc_csv = PathBuf::from("btc_30m.csv");
  let mut x_series =
    Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), x_ticker.clone())?;
  let eth_csv = PathBuf::from("eth_30m.csv");
  let mut y_series =
    Dataset::csv_series(&eth_csv, Some(start_time), Some(end_time), y_ticker.clone())?;

  Dataset::align(&mut x_series, &mut y_series)?;

  // normalize data using percent change from first price in time series
  let x = x_series.clone().normalize_series()?;
  let y = y_series.clone().normalize_series()?;
  assert_eq!(x.len(), y.len());

  // discover spread attribution (how much x and/or y contributed to the change in spread)
  let hedge_ratio: Vec<f64> = dynamic_hedge_kalman_filter(&x.y(), &y.y()).unwrap();
  println!("hedge ratio len: {}", hedge_ratio.len());
  assert_eq!(hedge_ratio.len(), x.len());
  assert_eq!(hedge_ratio.len(), y.len());
  assert_eq!(x.len(), y.len());

  let hedge_ratio = Dataset::new(
    hedge_ratio
      .iter()
      .enumerate()
      .map(|(i, y)| Data { x: x.x()[i], y: *y })
      .collect(),
  );

  let mut y_pos_attr = vec![];
  let mut y_neg_attr = vec![];
  let mut y_signed_attr = vec![];
  let mut hx_pos_attr = vec![];
  let mut hx_neg_attr = vec![];
  let mut hx_signed_attr = vec![];
  let mut y_diffs = vec![];
  let mut hx_diffs = vec![];
  let mut s_diffs = vec![];

  for ((x_window, y_window), hr_window) in x
    .data()
    .clone()
    .windows(2)
    .zip(y.data().clone().windows(2))
    .zip(hedge_ratio.data().clone().windows(2))
  {
    let x1 = x_window[0].clone();
    let x = x_window[1].clone();
    let y1 = y_window[0].clone();
    let y = y_window[1].clone();
    let hr1 = hr_window[0].clone();
    let hr = hr_window[1].clone();
    let date = x.x(); // equal to y.date and hr.date

    // hx is -hedge_ratio * x
    let hx1 = -hr1.y() * x1.y();
    let hx = -hr.y() * x.y();
    // spread = y - hedge_ratio * x
    let s1 = y1.y() - hx1;
    let s = y.y() - hx;
    let ds = s - s1;
    // change due to "x" is really -hedge_ratio * x
    let dhx = hx - hx1;
    let dy = y.y() - y1.y();

    let sign = if ds > 0.0 { 1.0 } else { -1.0 };

    // determine change in spread due to change in x and y as percentage
    let cumd = dy.abs() + dhx.abs();
    let y_pct = dy.abs() / cumd * 100.0;
    let hx_pct = dhx.abs() / cumd * 100.0;

    y_signed_attr.push(Data {
      x: date,
      y: y_pct * sign,
    });
    if sign == 1.0 {
      y_pos_attr.push(Data { x: date, y: y_pct });
    } else {
      y_neg_attr.push(Data { x: date, y: y_pct });
    }
    hx_signed_attr.push(Data {
      x: date,
      y: hx_pct * sign,
    });
    if sign == 1.0 {
      hx_pos_attr.push(Data { x: date, y: hx_pct });
    } else {
      hx_neg_attr.push(Data { x: date, y: hx_pct });
    }
    y_diffs.push(Data { x: date, y: dy });
    hx_diffs.push(Data { x: date, y: dhx });
    s_diffs.push(Data { x: date, y: ds });
  }

  // blue is y_attr, red is hx_attr
  Plot::plot(
    vec![y_signed_attr, hx_signed_attr],
    "btc_eth_signed_attributes.png",
    "BTC & ETH Spread Attribution",
    "% Attribution",
    "Unix Millis",
  )?;
  Plot::plot(
    vec![y_pos_attr, hx_pos_attr],
    "btc_eth_pos_attributes.png",
    "BTC & ETH Spread Attribution",
    "% Attribution",
    "Unix Millis",
  )?;
  Plot::plot(
    vec![y_neg_attr, hx_neg_attr],
    "btc_eth_neg_attributes.png",
    "BTC & ETH Spread Attribution",
    "% Attribution",
    "Unix Millis",
  )?;
  // blue is y_diffs, red is hx_diffs, green is s_diffs
  Plot::plot(
    vec![y_diffs, hx_diffs, s_diffs],
    "btc_eth_diffs.png",
    "BTC & ETH Spread Diffs",
    "Change",
    "Unix Millis",
  )?;

  // blue is ETH, red is BTC
  Plot::plot(
    vec![y.data().clone(), x.data().clone()],
    "btc_eth_normalized.png",
    "BTC & ETH Normalized Prices",
    "% Change from Origin",
    "Unix Millis",
  )?;

  Ok(())
}

// ==========================================================================================
//                                 1h Backtests
// ==========================================================================================

#[tokio::test]
async fn btc_eth_1h_stat_arb() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let start_time = Time::new(2017, 8, 1, None, None, None);
  let end_time = Time::new(2021, 8, 1, None, None, None);

  let window = 9;
  let capacity = window + 1;
  let threshold = 2.0;
  let stop_loss = None; //Some(5.0);
  let fee = 0.025;
  let slippage = 0.01;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = true;

  let x_ticker = "BTCUSDT".to_string();
  let y_ticker = "ETHUSDT".to_string();

  let btc_csv = workspace_path("data/btc_1h.csv");
  let mut x_series =
    Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), x_ticker.clone())?;

  let eth_csv = workspace_path("data/eth_1h.csv");
  let mut y_series =
    Dataset::csv_series(&eth_csv, Some(start_time), Some(end_time), y_ticker.clone())?;

  Dataset::align(&mut x_series, &mut y_series)?;
  assert_eq!(x_series.len(), y_series.len());

  let strat = StatArbBacktest::new(
    capacity,
    window,
    threshold,
    x_ticker.clone(),
    y_ticker.clone(),
    stop_loss,
  );
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
    backtest.get_series(&x_ticker)?.len()
  );
  println!(
    "Backtest ETH candles: {}",
    backtest.get_series(&y_ticker)?.len()
  );

  let now = Time::now();
  let summary = backtest.backtest()?;
  let all_buy_and_hold = backtest.buy_and_hold()?;
  println!(
    "backtest finished in {}s",
    Time::now().to_unix() - now.to_unix()
  );

  if let Ok(trades) = backtest.get_trades(&x_ticker) {
    if trades.len() > 1 {
      summary.print(&x_ticker);
      let x_bah = all_buy_and_hold
        .get(&x_ticker)
        .ok_or(anyhow::anyhow!("Buy and hold not found for ticker"))?
        .clone();
      Plot::plot(
        vec![summary.cum_pct(&x_ticker)?.data().clone(), x_bah],
        "btc_1h_backtest.png",
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
        "eth_1h_backtest.png",
        &format!("{} Stat Arb Backtest", y_ticker),
        "% ROI",
        "Unix Millis",
      )?;
    }
  }

  Ok(())
}

#[tokio::test]
async fn btc_eth_1h_cointegration() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let x_ticker = "BTCUSDT".to_string();
  let y_ticker = "ETHUSDT".to_string();

  let mut backtest = Backtest::default();
  let btc_csv = workspace_path("data/btc_1h.csv");
  let mut x_series = Dataset::csv_series(&btc_csv, None, None, x_ticker.clone())?;
  let eth_csv = workspace_path("data/eth_1h.csv");
  let mut y_series = Dataset::csv_series(&eth_csv, None, None, y_ticker.clone())?;

  Dataset::align(&mut x_series, &mut y_series)?;
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

  // normalize data using percent change from first price in time series
  let x = Dataset::new(backtest.get_series(&x_ticker)?.clone()).normalize_series()?;
  let y = Dataset::new(backtest.get_series(&y_ticker)?.clone()).normalize_series()?;
  assert_eq!(x.len(), y.len());

  let coint = engle_granger_cointegration_test(&x.y(), &y.y())
    .map_err(|e| anyhow::anyhow!("Error calculating Engle-Granger cointegration test: {}", e))?;
  println!("Cointegration Test: {:#?}", coint);

  let spread: Vec<f64> = spread_standard(&x.y(), &y.y())
    .map_err(|e| anyhow::anyhow!("Error calculating dynamic spread: {}", e))?;
  assert_eq!(spread.len(), y.len());
  assert_eq!(spread.len(), x.len());

  let half_life = half_life(&spread).unwrap().abs().round() as usize;
  println!("Spread half life: {}", half_life);
  let window = 100;

  let roll_coint = Dataset::new(
    rolling_cointegration(&x.y(), &y.y(), window)
      .map_err(|e| anyhow::anyhow!("Error calculating rolling cointegration: {}", e))?
      .iter()
      .enumerate()
      .map(|(i, x)| Data { x: i as i64, y: *x })
      .collect(),
  );
  Plot::plot(
    vec![roll_coint.0],
    "btc_eth_1h_coint.png",
    "BTC/ETH Cointegration",
    "Cointegration",
    "Time",
  )?;

  let roll_corr = rolling_correlation(&x.y(), &y.y(), window)
    .map_err(|e| anyhow::anyhow!("Error calculating rolling correlation: {}", e))?;
  assert_eq!(roll_corr.len(), y.len());
  assert_eq!(roll_corr.len(), x.len());
  let correlation = Dataset::new(
    roll_corr
      .iter()
      .enumerate()
      .map(|(i, x)| Data { x: i as i64, y: *x })
      .collect(),
  );
  Plot::plot(
    vec![correlation.0],
    "btc_eth_1h_corr.png",
    "BTC/ETH Correlation",
    "Correlation",
    "Time",
  )?;

  let spread_dataset = Dataset::new(
    spread
      .iter()
      .enumerate()
      .map(|(i, x)| Data { x: i as i64, y: *x })
      .collect(),
  );
  Plot::plot(
    vec![spread_dataset.0],
    "btc_eth_1h_spread.png",
    "BTC/ETH Spread",
    "Spread",
    "Time",
  )?;

  let zscore: Vec<f64> = rolling_zscore(&spread, window).unwrap();
  assert_eq!(zscore.len(), spread.len());
  let zscore = Dataset::new(
    zscore
      .iter()
      .enumerate()
      .map(|(i, x)| Data { x: i as i64, y: *x })
      .collect(),
  );
  Plot::plot(
    vec![zscore.0],
    "btc_eth_1h_spread_zscore.png",
    "BTC/ETH Spread Z-Score",
    "Z-Score",
    "Time",
  )?;

  Ok(())
}

// ==========================================================================================
//                                 1d Backtests
// ==========================================================================================

#[tokio::test]
async fn btc_eth_1d_stat_arb() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let window = 9;
  let capacity = window + 1;
  let threshold = 2.0;
  let stop_loss = None; //Some(5.0);
  let fee = 0.025;
  let slippage = 0.01;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = false;

  let x_ticker = "BTCUSDT".to_string();
  let y_ticker = "ETHUSDT".to_string();

  let btc_csv = workspace_path("data/btc_1d.csv");
  let mut x_series = Dataset::csv_series(&btc_csv, None, None, x_ticker.clone())?;

  let eth_csv = workspace_path("data/eth_1d.csv");
  let mut y_series = Dataset::csv_series(&eth_csv, None, None, y_ticker.clone())?;

  Dataset::align(&mut x_series, &mut y_series)?;
  assert_eq!(x_series.len(), y_series.len());

  let strat = StatArbBacktest::new(
    capacity,
    window,
    threshold,
    x_ticker.clone(),
    y_ticker.clone(),
    stop_loss,
  );
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
    backtest.get_series(&x_ticker)?.len()
  );
  println!(
    "Backtest ETH candles: {}",
    backtest.get_series(&y_ticker)?.len()
  );

  let now = Time::now();
  let summary = backtest.backtest()?;
  let all_buy_and_hold = backtest.buy_and_hold()?;
  println!(
    "backtest finished in {}s",
    Time::now().to_unix() - now.to_unix()
  );

  if let Ok(trades) = backtest.get_trades(&x_ticker) {
    if trades.len() > 1 {
      summary.print(&x_ticker);
      let x_bah = all_buy_and_hold
        .get(&x_ticker)
        .ok_or(anyhow::anyhow!("Buy and hold not found for ticker"))?
        .clone();
      Plot::plot(
        vec![summary.cum_pct(&x_ticker)?.data().clone(), x_bah],
        "btc_1d_backtest.png",
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
        "eth_1d_backtest.png",
        &format!("{} Stat Arb Backtest", y_ticker),
        "% ROI",
        "Unix Millis",
      )?;
    }
  }

  Ok(())
}

#[tokio::test]
async fn btc_eth_1d_cointegration() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let x_ticker = "BTCUSDT".to_string();
  let y_ticker = "ETHUSDT".to_string();

  let mut backtest = Backtest::default();
  let btc_csv = workspace_path("data/btc_1d.csv");
  let mut x_series = Dataset::csv_series(&btc_csv, None, None, x_ticker.clone())?;
  let eth_csv = workspace_path("data/eth_1d.csv");
  let mut y_series = Dataset::csv_series(&eth_csv, None, None, y_ticker.clone())?;

  Dataset::align(&mut x_series, &mut y_series)?;
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

  // normalize data using percent change from first price in time series
  let x = Dataset::new(backtest.get_series(&x_ticker)?.clone()).normalize_series()?;
  let y = Dataset::new(backtest.get_series(&y_ticker)?.clone()).normalize_series()?;
  // let x = Dataset::new(backtest.get_series(&x_ticker)?.clone());
  // let y = Dataset::new(backtest.get_series(&y_ticker)?.clone());
  assert_eq!(x.len(), y.len());

  let coint = engle_granger_cointegration_test(&x.y(), &y.y())
    .map_err(|e| anyhow::anyhow!("Error calculating Engle-Granger cointegration test: {}", e))?;
  println!("Cointegration Test: {:#?}", coint);

  let spread: Vec<f64> = spread_standard(&x.y(), &y.y())
    .map_err(|e| anyhow::anyhow!("Error calculating dynamic spread: {}", e))?;
  assert_eq!(spread.len(), y.len());
  assert_eq!(spread.len(), x.len());

  let half_life = half_life(&spread).unwrap().abs().round() as usize;
  println!("Spread half life: {}", half_life);
  let window = 100;

  let roll_coint = Dataset::new(
    rolling_cointegration(&x.y(), &y.y(), window)
      .map_err(|e| anyhow::anyhow!("Error calculating rolling cointegration: {}", e))?
      .iter()
      .enumerate()
      .map(|(i, x)| Data { x: i as i64, y: *x })
      .collect(),
  );
  Plot::plot(
    vec![roll_coint.0],
    "btc_eth_1d_coint.png",
    "BTC/ETH Cointegration",
    "Cointegration",
    "Time",
  )?;

  let roll_corr = rolling_correlation(&x.y(), &y.y(), window)
    .map_err(|e| anyhow::anyhow!("Error calculating rolling correlation: {}", e))?;
  assert_eq!(roll_corr.len(), y.len());
  assert_eq!(roll_corr.len(), x.len());
  let correlation = Dataset::new(
    roll_corr
      .iter()
      .enumerate()
      .map(|(i, x)| Data { x: i as i64, y: *x })
      .collect(),
  );
  Plot::plot(
    vec![correlation.0],
    "btc_eth_1d_corr.png",
    "BTC/ETH Correlation",
    "Correlation",
    "Time",
  )?;

  let spread_dataset = Dataset::new(
    spread
      .iter()
      .enumerate()
      .map(|(i, x)| Data { x: i as i64, y: *x })
      .collect(),
  );
  Plot::plot(
    vec![spread_dataset.0],
    "btc_eth_1d_spread.png",
    "BTC/ETH Spread",
    "Spread",
    "Time",
  )?;

  let zscore: Vec<f64> = rolling_zscore(&spread, window).unwrap();
  assert_eq!(zscore.len(), spread.len());
  let zscore = Dataset::new(
    zscore
      .iter()
      .enumerate()
      .map(|(i, x)| Data { x: i as i64, y: *x })
      .collect(),
  );
  Plot::plot(
    vec![zscore.0],
    "btc_eth_1d_spread_zscore.png",
    "BTC/ETH Spread Z-Score",
    "Z-Score",
    "Time",
  )?;

  Ok(())
}
