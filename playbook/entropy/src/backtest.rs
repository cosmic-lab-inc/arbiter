#![allow(unused_imports)]

use crate::math::hurst;
use crate::trade::{Bet, Signal, SignalInfo};
use crate::{Backtest, Dataset, Strategy};
use log::warn;
use ndarray::AssignElem;
use nexus::*;
use rayon::prelude::*;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tradestats::kalman::*;
use tradestats::metrics::*;
use tradestats::utils::*;

#[derive(Debug, Clone)]
pub struct EntropyBacktest {
  pub period: usize,
  pub patterns: usize,
  pub cache: RingBuffer<Data>,
  pub stop_loss_pct: Option<f64>,
}

impl EntropyBacktest {
  pub fn new(
    capacity: usize,
    period: usize,
    patterns: usize,
    ticker: String,
    stop_loss_pct: Option<f64>,
  ) -> Self {
    Self {
      period,
      patterns,
      cache: RingBuffer::new(capacity, ticker),
      stop_loss_pct,
    }
  }

  pub fn signal(&mut self, ticker: Option<String>) -> anyhow::Result<Vec<Signal>> {
    match ticker {
      None => Ok(vec![]),
      Some(ticker) => {
        if self.cache.vec.len() < self.cache.capacity {
          return Ok(vec![]);
        }
        if ticker != self.cache.id {
          return Ok(vec![]);
        }

        let series = Dataset::new(self.cache.vec());

        let last_index = series.len() - 1;
        let mut upup = series.y().clone();
        let mut dndn = series.y().clone();
        let mut updn = series.y().clone();
        let mut dnup = series.y().clone();
        upup[1] = series.0[0].y + 1.0;
        upup[0] = upup[1] + 1.0;
        dndn[1] = series.0[0].y - 1.0;
        dndn[0] = dndn[1] - 1.0;
        updn[1] = series.0[0].y + 1.0;
        updn[0] = updn[1] - 1.0;
        dnup[1] = series.0[0].y - 1.0;
        dnup[0] = dnup[1] + 1.0;

        let entropy_upup = shannon_entropy(&upup, self.period + 1, self.patterns);
        let entropy_dndn = shannon_entropy(&dndn, self.period + 1, self.patterns);
        let entropy_updn = shannon_entropy(&updn, self.period + 1, self.patterns);
        let entropy_dnup = shannon_entropy(&dnup, self.period + 1, self.patterns);

        let _0: Data = series.0[last_index].clone();
        let _2: Data = series.0[last_index - 2].clone();

        let mut enter_long = false;
        let mut exit_long = false;
        let mut enter_short = false;
        let mut exit_short = false;

        let max = entropy_upup
          .max(entropy_dndn)
          .max(entropy_updn)
          .max(entropy_dnup);

        if max == entropy_upup && _2.y > _0.y {
          enter_long = true;
          exit_short = true;
        } else if max == entropy_dndn && _2.y < _0.y {
          enter_short = true;
          exit_long = true;
        }

        let latest_data = self
          .cache
          .front()
          .ok_or(anyhow::anyhow!("No data in cache"))?;
        let info = SignalInfo {
          price: latest_data.y(),
          date: Time::from_unix_ms(latest_data.x()),
          ticker: ticker.clone(),
        };
        let mut signals = vec![];
        if exit_short {
          signals.push(Signal::ExitShort(info.clone()));
        }
        if exit_long {
          signals.push(Signal::ExitLong(info.clone()));
        }
        if enter_short {
          signals.push(Signal::EnterShort(info.clone()));
        }
        if enter_long {
          signals.push(Signal::EnterLong(info));
        }
        Ok(signals)
      }
    }
  }
}

impl Strategy<Data> for EntropyBacktest {
  /// Appends candle to candle cache and returns a signal (long, short, or do nothing).
  fn process_data(
    &mut self,
    data: Data,
    ticker: Option<String>,
    _equity: Option<f64>,
  ) -> anyhow::Result<Vec<Signal>> {
    if let Some(ticker) = ticker.clone() {
      if ticker == self.cache.id {
        self.cache.push(Data {
          x: data.x,
          y: data.y,
        });
      }
    }

    self.signal(ticker)
  }

  fn cache(&self, ticker: Option<String>) -> Option<&RingBuffer<Data>> {
    if let Some(ticker) = ticker {
      if ticker == self.cache.id {
        Some(&self.cache)
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
    "entropy".to_string()
  }
}

// ==========================================================================================
//                                 Fast Fourier Transform
// ==========================================================================================

#[test]
fn fft() -> anyhow::Result<()> {
  use ndarray::Array1;
  use rustfft::algorithm::Radix4;
  use rustfft::num_complex::Complex;
  use rustfft::num_traits::Zero;
  use rustfft::{Fft, FftDirection, FftPlanner};

  let start_time = Time::new(2017, 7, 1, None, None, None);
  let end_time = Time::new(2024, 7, 1, None, None, None);
  let btc_csv = workspace_path("data/btc_1d.csv");
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?
    .normalize_series()?;

  // Assuming df['Close'] is a Vec<f64>
  let close_prices: Vec<f64> = btc_series.y();

  let fft_len = close_prices.len();
  let mut planner = FftPlanner::new();
  let fft = planner.plan_fft_forward(fft_len);

  //
  // Convert to FFT frequencies
  //

  let mut fft_input: Vec<Complex<f64>> = close_prices
    .into_iter()
    .map(|x| Complex::new(x, 0.0))
    .collect();
  // Perform FFT
  fft.process(&mut fft_input);

  // Calculate FFT frequencies
  let sample_spacing = 1.0; // Assuming daily data, d=1
  let frequencies: Vec<f64> = fftfreq(fft_len, sample_spacing);
  // Calculate magnitude
  let magnitude: Vec<f64> = fft_input.iter().map(|x| x.norm()).collect();
  // Calculate periods
  let periods: Array1<f64> = 1.0 / Array1::from(frequencies.clone());

  //
  // Reconstruct time series from FFT frequencies (inverse FFT)
  //

  let mut ifft_input = fft_input.clone();
  let ifft = planner.plan_fft_inverse(fft_len);
  ifft.process(&mut ifft_input);

  // The input vector now contains the IFFT result, which should be close to the original time series
  let recovered: Vec<f64> = ifft_input.iter().map(|x| x.re).collect();

  // Now you can plot the recovered data
  let recovered_data: Vec<Data> = recovered
    .iter()
    .enumerate()
    .map(|(i, &y)| Data { x: i as i64, y })
    .collect();

  //
  // Reconstruct time series from dominant (top 25) FFT frequencies
  //

  let top_cutoff = 5;

  let mut top_ifft_input = fft_input.clone();
  let top_ifft = planner.plan_fft_inverse(fft_len);

  // top 25
  let mut dominant_periods: Vec<(f64, f64)> = periods
    .iter()
    .zip(magnitude.iter())
    .map(|(&x, &y)| (y, x)) // Swap to sort by magnitude
    .collect();

  // Sort by magnitude in descending order and take the top 25
  dominant_periods.sort_by(|a, b| b.partial_cmp(a).unwrap_or(Ordering::Equal));
  let dominant_periods: Vec<f64> = dominant_periods
    .into_iter()
    .map(|(_, period)| period)
    .take(top_cutoff)
    .collect();

  // Find the minimum period of the top 25
  let min_period = *dominant_periods
    .iter()
    .min_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal))
    .unwrap();
  println!("min period of top {}: {}", top_cutoff, min_period);

  let mut zeroed = 0;
  // Set the values to zero where the absolute value of the frequencies is greater than the inverse of the minimum of the top periods
  for (i, &freq) in frequencies.iter().enumerate() {
    if freq.abs() > 1.0 / min_period.abs() {
      zeroed += 1;
      top_ifft_input[i] = Complex::zero();
    }
  }
  println!("zeroed: {}", zeroed);

  top_ifft.process(&mut top_ifft_input);

  // The vector now contains the IFFT result of the top 25 (dominant) periods
  let top_recovered: Vec<f64> = top_ifft_input.iter().map(|x| x.re).collect();
  let top_recovered_data: Vec<Data> = top_recovered
    .iter()
    .enumerate()
    .map(|(i, &y)| Data { x: i as i64, y })
    .collect();

  Plot::plot(
    vec![top_recovered_data, recovered_data],
    "btc_ifft.png",
    &format!("{} Inverse FFT", ticker),
    "Price",
    "Time",
  )?;

  Ok(())
}

fn fftfreq(n: usize, d: f64) -> Vec<f64> {
  let val = 1.0 / (n as f64 * d);
  let mut result = Vec::with_capacity(n);
  let m = if n % 2 == 0 { n / 2 } else { n / 2 + 1 };
  for i in 0..m {
    result.push(i as f64 * val);
  }
  for i in -(n as i64 / 2)..0 {
    result.push(i as f64 * val);
  }
  result
}

// ==========================================================================================
//                                 Entropy
// ==========================================================================================

#[test]
fn test_shannon_entropy() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();
  let start_time = Time::new(2020, 7, 1, None, None, None);
  let end_time = Time::new(2024, 7, 1, None, None, None);
  let btc_csv = workspace_path("data/btc_1d.csv");
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 100;
  let patterns = 2;

  let mut results = vec![];
  for i in 0..btc_series.y().len() - period {
    let series = &btc_series.y()[i..i + period];
    let entropy = shannon_entropy(series, period, patterns);
    results.push(entropy);
  }
  let avg = results.iter().sum::<f64>() / results.len() as f64;
  let randomness = avg / (patterns as f64) * 100.0;
  println!(
    "bits: {}, entropy: {}, randomness: {}%",
    patterns, avg, randomness
  );

  Ok(())
}

#[test]
fn entropy_one_step_prediction() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();
  let clock_start = Time::now();
  let start_time = Time::new(2022, 7, 1, None, None, None);
  let end_time = Time::new(2024, 7, 1, None, None, None);
  let btc_csv = workspace_path("data/btc_1h.csv");
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 100;
  let patterns = 3;

  let mut win = 0;
  let mut loss = 0;
  let mut win_tot = 0.0;
  let mut loss_tot = 0.0;

  let mut entropies = vec![];
  for i in 0..btc_series.y().len() - period {
    let series = btc_series.y()[i..i + period].to_vec();
    let last_index = series.len() - 1;
    let mut up = series.clone();
    up[0] = series[0] + 1.0;
    let mut down = series.clone();
    down[0] = series[0] - 1.0;

    let entropy_up = shannon_entropy(&up, period + 1, patterns);
    let entropy_down = shannon_entropy(&down, period + 1, patterns);

    let _0 = series[last_index];
    let _1 = series[last_index - 1];

    if entropy_up > entropy_down && _1 > _0 {
      win += 1;
      win_tot += _1 - _0;
    } else if entropy_up < entropy_down && _1 < _0 {
      win += 1;
      win_tot = _0 - _1;
    } else if entropy_up > entropy_down && _1 < _0 {
      loss += 1;
      loss_tot += _1 - _0;
    } else if entropy_up < entropy_down && _1 > _0 {
      loss += 1;
      loss_tot += _0 - _1;
    }
    entropies.push(shannon_entropy(series.as_slice(), period, patterns));
  }
  let avg_entropy = entropies.iter().sum::<f64>() / entropies.len() as f64;
  println!("entropy: {}/{}", trunc!(avg_entropy, 3), patterns);

  println!(
    "trades: {}, win rate: {}%, profit: ${}",
    win + loss,
    trunc!(win as f64 / (win + loss) as f64 * 100.0, 3),
    trunc!(win_tot - loss_tot, 2)
  );

  println!(
    "finished test in: {}s",
    Time::now().to_unix() - clock_start.to_unix()
  );

  Ok(())
}

#[test]
fn entropy_two_step_prediction() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();
  let clock_start = Time::now();
  let start_time = Time::new(2017, 7, 1, None, None, None);
  let end_time = Time::new(2024, 7, 1, None, None, None);
  let btc_csv = workspace_path("data/btc_1h.csv");
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 100;
  let patterns = 3;

  let mut win = 0;
  let mut loss = 0;
  let mut cum_pnl = 0.0;

  let mut entropies = vec![];
  let mut pnl_series = vec![];
  for i in 0..btc_series.y().len() - period {
    let series = btc_series.y()[i..i + period].to_vec();
    let last_index = series.len() - 1;
    let mut upup = series.clone();
    let mut dndn = series.clone();
    let mut updn = series.clone();
    let mut dnup = series.clone();
    upup[1] = series[0] + 1.0;
    upup[0] = upup[1] + 1.0;
    dndn[1] = series[0] - 1.0;
    dndn[0] = dndn[1] - 1.0;
    updn[1] = series[0] + 1.0;
    updn[0] = updn[1] - 1.0;
    dnup[1] = series[0] - 1.0;
    dnup[0] = dnup[1] + 1.0;

    let entropy_upup = shannon_entropy(&upup, period + 1, patterns);
    let entropy_dndn = shannon_entropy(&dndn, period + 1, patterns);
    let entropy_updn = shannon_entropy(&updn, period + 1, patterns);
    let entropy_dnup = shannon_entropy(&dnup, period + 1, patterns);

    let _0 = series[last_index];
    let _2 = series[last_index - 2];

    let max = entropy_upup
      .max(entropy_dndn)
      .max(entropy_updn)
      .max(entropy_dnup);

    if max == entropy_upup && _2 > _0 {
      win += 1;
      cum_pnl += _2 - _0;
    } else if max == entropy_dndn && _2 < _0 {
      win += 1;
      cum_pnl += _0 - _2;
    } else if max == entropy_upup && _2 < _0 {
      loss += 1;
      cum_pnl -= _0 - _2;
    } else if max == entropy_dndn && _2 > _0 {
      loss += 1;
      cum_pnl -= _2 - _0;
    }
    pnl_series.push(cum_pnl);
    entropies.push(shannon_entropy(series.as_slice(), period, patterns));
  }
  let avg_entropy = entropies.iter().sum::<f64>() / entropies.len() as f64;
  println!("entropy: {}/{}", trunc!(avg_entropy, 3), patterns);

  println!(
    "trades: {}, win rate: {}%, profit: ${}",
    win + loss,
    trunc!(win as f64 / (win + loss) as f64 * 100.0, 3),
    trunc!(cum_pnl, 2)
  );

  println!(
    "finished test in: {}s",
    Time::now().to_unix() - clock_start.to_unix()
  );

  let pnl_series = Dataset::new(
    pnl_series
      .into_iter()
      .enumerate()
      .map(|(i, pnl)| Data {
        x: i as i64,
        y: pnl,
      })
      .collect(),
  );
  Plot::plot(
    vec![pnl_series.0],
    "btc_entropy.png",
    "BTC Entropy",
    "$ PnL",
    "Time",
  )?;

  Ok(())
}

// ==========================================================================================
//                                 Backtest
// ==========================================================================================

#[test]
fn entropy_backtest() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let stop_loss = None;
  let fee = 0.0;
  let slippage = 0.0;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = false;

  let start_time = Time::new(2017, 7, 1, None, None, None);
  let end_time = Time::new(2024, 7, 1, None, None, None);
  let timeframe = "1h";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 100;
  let capacity = period + 1;
  let patterns = 3;

  let strat = EntropyBacktest::new(capacity, period, patterns, ticker.clone(), stop_loss);
  let mut backtest = Backtest::builder(strat)
    .fee(fee)
    .slippage(slippage)
    .bet(bet)
    .leverage(leverage)
    .short_selling(short_selling);

  backtest
    .series
    .insert(ticker.clone(), series.data().clone());

  backtest.execute("Entropy Backtest", timeframe)?;

  Ok(())
}
