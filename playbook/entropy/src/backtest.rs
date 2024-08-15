#![allow(unused_imports)]

use crate::math::hurst;
use crate::trade::{Bet, Signal, SignalInfo};
use crate::{Backtest, Dataset, Strategy};
use log::warn;
use ndarray::AssignElem;
use nexus::*;
use rayon::prelude::*;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use tradestats::kalman::*;
use tradestats::metrics::*;
use tradestats::utils::*;

/// Based on this blog: https://robotwealth.com/shannon-entropy/
#[derive(Debug, Clone)]
pub struct EntropyBacktest {
  pub period: usize,
  pub patterns: usize,
  pub cache: RingBuffer<Data>,
  assets: Assets,
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
      assets: Assets::default(),
      stop_loss_pct,
    }
  }

  fn generate_signals(&self) -> anyhow::Result<Signals> {
    let series = Dataset::new(self.cache.vec());

    let mut b11 = series.y().clone();
    let mut b00 = series.y().clone();
    let mut b10 = series.y().clone();
    let mut b01 = series.y().clone();

    b11[1] = series.0[0].y + 1.0;
    b11[0] = b11[1] + 1.0;

    b00[1] = series.0[0].y - 1.0;
    b00[0] = b00[1] - 1.0;

    b10[1] = series.0[0].y + 1.0;
    b10[0] = b10[1] - 1.0;

    b01[1] = series.0[0].y - 1.0;
    b01[0] = b01[1] + 1.0;

    let entropy_b11 = shannon_entropy(&b11, self.period + 1, self.patterns);
    let entropy_b00 = shannon_entropy(&b00, self.period + 1, self.patterns);
    let entropy_b10 = shannon_entropy(&b10, self.period + 1, self.patterns);
    let entropy_b01 = shannon_entropy(&b01, self.period + 1, self.patterns);

    let last_index = series.len() - 1;
    let p0 = &series.0[last_index];
    let p2 = &series.0[last_index - 2];

    let mut enter_long = false;
    let mut exit_long = false;
    let mut enter_short = false;
    let mut exit_short = false;

    let max = entropy_b11
      .max(entropy_b00)
      .max(entropy_b10)
      .max(entropy_b01);

    // original
    if max == entropy_b11 && p2.y > p0.y {
      enter_long = true;
      exit_short = true;
    } else if max == entropy_b00 && p2.y < p0.y {
      enter_short = true;
      exit_long = true;
    }

    // if max == entropy_b11 && p2.y > p0.y {
    //   enter_short = true;
    //   exit_long = true;
    // } else if max == entropy_b00 && p2.y < p0.y {
    //   enter_long = true;
    //   exit_short = true;
    // }

    Ok(Signals {
      enter_long,
      exit_long,
      enter_short,
      exit_short,
    })
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

        let Signals {
          enter_long,
          exit_long,
          enter_short,
          exit_short,
        } = self.generate_signals()?;

        let latest_data = self
          .cache
          .front()
          .ok_or(anyhow::anyhow!("No data in cache"))?;

        let enter_info = SignalInfo {
          price: latest_data.y(),
          date: Time::from_unix_ms(latest_data.x()),
          ticker: ticker.clone(),
          quantity: self.assets.cash()?.quantity / latest_data.y(),
        };
        let exit_info = SignalInfo {
          price: latest_data.y(),
          date: Time::from_unix_ms(latest_data.x()),
          ticker: ticker.clone(),
          quantity: self.assets.get_or_err(&ticker)?.quantity,
        };

        let mut signals = vec![];
        if exit_short {
          signals.push(Signal::ExitShort(exit_info.clone()));
        }
        if exit_long {
          signals.push(Signal::ExitLong(exit_info.clone()));
        }
        if enter_short {
          signals.push(Signal::EnterShort(enter_info.clone()));
        }
        if enter_long {
          signals.push(Signal::EnterLong(enter_info));
        }
        Ok(signals)
      }
    }
  }
}

impl Strategy<Data> for EntropyBacktest {
  fn process_data(
    &mut self,
    data: Data,
    ticker: Option<String>,
    assets: &Assets,
  ) -> anyhow::Result<Vec<Signal>> {
    if let Some(ticker) = ticker.clone() {
      if ticker == self.cache.id {
        self.cache.push(Data {
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

/// Reference: https://medium.com/@kt.26karanthakur/stock-market-signal-analysis-using-fast-fourier-transform-e3bdde7bcee6
/// Research: https://www.math.utah.edu/~gustafso/s2017/2270/projects-2016/williamsBarrett/williamsBarrett-Fast-Fourier-Transform-Predicting-Financial-Securities-Prices.pdf
#[test]
fn fft() -> anyhow::Result<()> {
  use ndarray::Array1;
  use rustfft::algorithm::Radix4;
  use rustfft::num_complex::Complex;
  use rustfft::num_traits::Zero;
  use rustfft::{Fft, FftDirection, FftPlanner};

  let start_time = Time::new(2020, 7, 1, None, None, None);
  let end_time = Time::new(2022, 7, 1, None, None, None);
  let timeframe = "1h";
  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

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
  let frequencies: Vec<f64> = fft_frequencies(fft_len, sample_spacing);
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

  let top_cutoff = 25;

  let mut top_ifft_input = fft_input.clone();
  let top_ifft = planner.plan_fft_inverse(fft_len);

  struct Freq {
    mag: f64,
    period: f64,
  }

  let mut sorted_freq: Vec<Freq> = periods
    .iter()
    .zip(magnitude.iter())
    .map(|(&x, &y)| Freq { mag: y, period: x }) // Swap to sort by magnitude
    .collect();

  // Sort by magnitude in descending order and take the top 25
  sorted_freq.sort_by(|a, b| b.period.partial_cmp(&a.period).unwrap_or(Ordering::Equal));

  let dominant_periods: Vec<f64> = sorted_freq
    .into_iter()
    .map(|freq| freq.period)
    .take(top_cutoff)
    .collect();

  // Find the minimum period of the top 25
  let min_period = *dominant_periods
    .iter()
    .min_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal))
    .unwrap();
  println!("min period of top {}: {}", top_cutoff, min_period);

  // Set the values to zero where the absolute value of the frequencies is greater than the inverse of the minimum of the top periods
  for (i, &freq) in frequencies.iter().enumerate() {
    if freq.abs() > 1.0 / min_period {
      top_ifft_input[i] = Complex::zero();
    }
  }

  top_ifft.process(&mut top_ifft_input);

  // The vector now contains the IFFT result of the top 25 (dominant) periods
  let top_recovered: Vec<f64> = top_ifft_input.iter().map(|x| x.re).collect();
  let top_recovered_data: Vec<Data> = top_recovered
    .iter()
    .enumerate()
    .map(|(i, &y)| Data { x: i as i64, y })
    .collect();

  let period = 100;
  let patterns = 3;

  let recovered_dataset = Dataset::new(recovered_data.clone()).y();
  let original_entropy = shannon_entropy(recovered_dataset.as_slice(), period + 1, patterns);
  println!(
    "original entropy: {}/{}",
    trunc!(original_entropy, 3),
    patterns
  );

  let top_recovered_dataset = Dataset::new(top_recovered_data.clone()).y();
  let model_entropy = shannon_entropy(top_recovered_dataset.as_slice(), period + 1, patterns);
  println!("model entropy: {}/{}", trunc!(model_entropy, 3), patterns);

  Plot::plot_without_legend(
    vec![recovered_data, top_recovered_data],
    "btc_ifft.png",
    &format!("{} Inverse FFT", ticker),
    "Price",
    "Time",
  )?;

  Ok(())
}

// ==========================================================================================
//                                 Entropy
// ==========================================================================================

#[test]
fn entropy_one_step() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();
  let clock_start = Time::now();
  let start_time = Time::new(2017, 7, 1, None, None, None);
  let end_time = Time::new(2024, 7, 1, None, None, None);
  let btc_csv = workspace_path("data/btc_1h.csv");
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 100;
  let patterns = 2;

  let mut win = 0;
  let mut loss = 0;
  let mut cum_pnl = 0.0;

  let mut entropies = vec![];
  let mut pnl_series = vec![];
  for i in 0..btc_series.y().len() - period {
    let series = btc_series.y()[i..i + period].to_vec();
    let last_index = series.len() - 1;

    let mut b1 = series.clone();
    let mut b0 = series.clone();

    b1[0] = series[0] + 1.0;
    b0[0] = series[0] - 1.0;

    let entropy_b1 = shannon_entropy(&b1, period + 1, patterns);
    let entropy_b0 = shannon_entropy(&b0, period + 1, patterns);

    let p0 = series[last_index];
    let p1 = series[last_index - 1];

    if entropy_b1 > entropy_b0 && p1 > p0 {
      win += 1;
      cum_pnl += p1 - p0;
    } else if entropy_b1 < entropy_b0 && p1 < p0 {
      win += 1;
      cum_pnl += p0 - p1;
    } else if entropy_b1 > entropy_b0 && p1 < p0 {
      loss += 1;
      cum_pnl -= p1 - p0;
    } else if entropy_b1 < entropy_b0 && p1 > p0 {
      loss += 1;
      cum_pnl -= p0 - p1;
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
    vec![Series {
      data: pnl_series.0,
      label: "Strategy".to_string(),
    }],
    "btc_one_step_entropy.png",
    "BTC Entropy",
    "$ PnL",
    "Time",
  )?;

  Ok(())
}

#[test]
fn entropy_two_step() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();
  let clock_start = Time::now();
  let start_time = Time::new(2022, 1, 1, None, None, None);
  let end_time = Time::new(2022, 7, 1, None, None, None);

  let timeframe = "1m";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
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
    let mut b11 = series.clone();
    let mut b00 = series.clone();
    let mut b10 = series.clone();
    let mut b01 = series.clone();

    b11[1] = series[0] + 1.0;
    b11[0] = b11[1] + 1.0;

    b00[1] = series[0] - 1.0;
    b00[0] = b00[1] - 1.0;

    b10[1] = series[0] + 1.0;
    b10[0] = b10[1] - 1.0;

    b01[1] = series[0] - 1.0;
    b01[0] = b01[1] + 1.0;

    let entropy_b11 = shannon_entropy(&b11, period + 1, patterns);
    let entropy_b00 = shannon_entropy(&b00, period + 1, patterns);
    let entropy_b10 = shannon_entropy(&b10, period + 1, patterns);
    let entropy_b01 = shannon_entropy(&b01, period + 1, patterns);

    let last_index = series.len() - 1;
    let p0 = series[last_index];
    let p2 = series[last_index - 2];

    let max = entropy_b11
      .max(entropy_b00)
      .max(entropy_b10)
      .max(entropy_b01);

    if max == entropy_b11 && p2 > p0 {
      win += 1;
      cum_pnl += p2 - p0;
    } else if max == entropy_b00 && p2 < p0 {
      win += 1;
      cum_pnl += p0 - p2;
    } else if max == entropy_b11 && p2 < p0 {
      loss += 1;
      cum_pnl -= p0 - p2;
    } else if max == entropy_b00 && p2 > p0 {
      loss += 1;
      cum_pnl -= p2 - p0;
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
    vec![Series {
      data: pnl_series.0,
      label: "Strategy".to_string(),
    }],
    "btc_two_step_entropy.png",
    "BTC Entropy",
    "$ PnL",
    "Time",
  )?;

  Ok(())
}

#[test]
fn entropy_three_step() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();
  let clock_start = Time::now();
  let start_time = Time::new(2017, 7, 1, None, None, None);
  let end_time = Time::new(2024, 7, 1, None, None, None);
  let btc_csv = workspace_path("data/btc_1h.csv");
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 100;
  let patterns = 4;

  let mut win = 0;
  let mut loss = 0;
  let mut cum_pnl = 0.0;

  let mut entropies = vec![];
  let mut pnl_series = vec![];
  for i in 0..btc_series.y().len() - period {
    let series = btc_series.y()[i..i + period].to_vec();
    let last_index = series.len() - 1;

    let mut b111 = series.clone();
    let mut b000 = series.clone();
    let mut b110 = series.clone();
    let mut b011 = series.clone();
    let mut b101 = series.clone();
    let mut b010 = series.clone();
    let mut b100 = series.clone();
    let mut b001 = series.clone();

    // bit representation is backwards, so 001, would be [2] = 0, [1] = 0, [0] = 1

    b111[2] = series[0] + 1.0;
    b111[1] = b111[2] + 1.0;
    b111[0] = b111[1] + 1.0;

    b000[2] = series[0] - 1.0;
    b000[1] = b000[2] - 1.0;
    b000[0] = b000[1] - 1.0;

    b110[2] = series[0] + 1.0;
    b110[1] = b110[2] + 1.0;
    b110[0] = b110[1] - 1.0;

    b011[2] = series[0] - 1.0;
    b011[1] = b011[2] + 1.0;
    b011[0] = b011[1] + 1.0;

    b101[2] = series[0] + 1.0;
    b101[1] = b101[2] - 1.0;
    b101[0] = b101[1] + 1.0;

    b010[2] = series[0] - 1.0;
    b010[1] = b010[2] + 1.0;
    b010[0] = b010[1] - 1.0;

    b100[2] = series[0] + 1.0;
    b100[1] = b100[2] - 1.0;
    b100[0] = b100[1] - 1.0;

    b001[2] = series[0] - 1.0;
    b001[1] = b001[2] - 1.0;
    b001[0] = b001[1] + 1.0;

    let entropy_b111 = shannon_entropy(&b111, period + 1, patterns);
    let entropy_b000 = shannon_entropy(&b000, period + 1, patterns);
    let entropy_b110 = shannon_entropy(&b110, period + 1, patterns);
    let entropy_b011 = shannon_entropy(&b011, period + 1, patterns);
    let entropy_b101 = shannon_entropy(&b101, period + 1, patterns);
    let entropy_b010 = shannon_entropy(&b010, period + 1, patterns);
    let entropy_b100 = shannon_entropy(&b100, period + 1, patterns);
    let entropy_b001 = shannon_entropy(&b001, period + 1, patterns);

    let p0 = series[last_index];
    let p3 = series[last_index - 3];

    let max = entropy_b111
      .max(entropy_b000)
      .max(entropy_b110)
      .max(entropy_b011)
      .max(entropy_b101)
      .max(entropy_b010)
      .max(entropy_b100)
      .max(entropy_b001);

    if max == entropy_b111 && p3 > p0 {
      win += 1;
      cum_pnl += p3 - p0;
    } else if max == entropy_b000 && p3 < p0 {
      win += 1;
      cum_pnl += p0 - p3;
    } else if max == entropy_b111 && p3 < p0 {
      loss += 1;
      cum_pnl -= p0 - p3;
    } else if max == entropy_b000 && p3 > p0 {
      loss += 1;
      cum_pnl -= p3 - p0;
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
    vec![Series {
      data: pnl_series.0,
      label: "Strategy".to_string(),
    }],
    "btc_three_step_entropy.png",
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
fn optimize_entropy_backtest() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let fee = 0.05;
  let slippage = 0.0;
  let stop_loss = None;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = true;

  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2024, 7, 1, None, None, None);
  let timeframe = "1d";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let patterns = 3;

  let summaries: Vec<(usize, Summary)> = (patterns..1_000)
    .into_par_iter()
    .flat_map(|period| {
      let strat = EntropyBacktest::new(period + 1, period, patterns, ticker.clone(), stop_loss);
      let mut backtest = Backtest::builder(strat)
        .fee(fee)
        .slippage(slippage)
        .bet(bet)
        .leverage(leverage)
        .short_selling(short_selling);

      backtest
        .series
        .insert(ticker.clone(), series.data().clone());

      Result::<_, anyhow::Error>::Ok((period, backtest.backtest()?))
    })
    .collect();

  // summary with the best roi
  {
    let (period, summary) = summaries
      .iter()
      .max_by(|(_, a), (_, b)| a.pct_roi(&ticker).partial_cmp(&b.pct_roi(&ticker)).unwrap())
      .unwrap();
    println!("--- Top by ROI ---");
    println!(
      "period: {}, roi: {}%, sharpe: {}, dd: {}%",
      period,
      summary.pct_roi(&ticker),
      summary.sharpe_ratio(&ticker, Timeframe::OneDay),
      summary.max_drawdown(&ticker)
    );
  }

  // summary with the best sharpe ratio
  {
    let (period, summary) = summaries
      .iter()
      .max_by(|(_, a), (_, b)| {
        a.sharpe_ratio(&ticker, Timeframe::OneDay)
          .partial_cmp(&b.sharpe_ratio(&ticker, Timeframe::OneDay))
          .unwrap()
      })
      .unwrap();
    println!("--- Top by Sharpe ---");
    println!(
      "period: {}, roi: {}%, sharpe: {}, dd: {}%",
      period,
      summary.pct_roi(&ticker),
      summary.sharpe_ratio(&ticker, Timeframe::OneDay),
      summary.max_drawdown(&ticker)
    );
  }

  // summary with the best drawdown
  {
    let (period, summary) = summaries
      .iter()
      .max_by(|(_, a), (_, b)| {
        a.max_drawdown(&ticker)
          .partial_cmp(&b.max_drawdown(&ticker))
          .unwrap()
      })
      .unwrap();
    println!("--- Top by Drawdown ---");
    println!(
      "period: {}, roi: {}%, sharpe: {}, dd: {}%",
      period,
      summary.pct_roi(&ticker),
      summary.sharpe_ratio(&ticker, Timeframe::OneDay),
      summary.max_drawdown(&ticker)
    );
  }

  Ok(())
}

#[test]
fn entropy_backtest() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let fee = 0.25;
  let slippage = 0.0;
  let stop_loss = None;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = true;

  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2024, 7, 1, None, None, None);
  let timeframe = "1d";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 104;
  let patterns = 3;

  let strat = EntropyBacktest::new(period + 1, period, patterns, ticker.clone(), stop_loss);
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
