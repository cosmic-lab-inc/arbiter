#![allow(unused_imports)]

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

#[derive(Debug, Clone)]
pub struct EntropyBacktest {
  period: usize,
  entropy_bits: EntropyBits,
  entropy_zscore_cutoff: Option<f64>,
  pub cache: RingBuffer<Data>,
  assets: Assets,
  pub stop_loss_pct: Option<f64>,
}

impl EntropyBacktest {
  pub fn new(
    period: usize,
    entropy_bits: EntropyBits,
    entropy_zscore_cutoff: Option<f64>,
    ticker: String,
    stop_loss_pct: Option<f64>,
  ) -> Self {
    Self {
      period,
      entropy_bits,
      entropy_zscore_cutoff,
      cache: RingBuffer::new(period + 1, ticker),
      assets: Assets::default(),
      stop_loss_pct,
    }
  }

  fn generate_signals(&self) -> anyhow::Result<Signals> {
    let mut enter_long = false;
    let mut exit_long = false;
    let mut enter_short = false;
    let mut exit_short = false;

    let series = Dataset::new(self.cache.vec());

    let y_series = series.y();
    let signal = match self.entropy_bits {
      EntropyBits::One => one_step_entropy_signal(series, self.period)?,
      EntropyBits::Two => two_step_entropy_signal(series, self.period)?,
      EntropyBits::Three => three_step_entropy_signal(series, self.period)?,
    };

    match self.entropy_zscore_cutoff {
      Some(cutoff) => {
        let entropy_zscore = zscore(y_series.as_slice(), self.period)?;
        if entropy_zscore.abs() > cutoff {
          match signal {
            EntropySignal::Up => {
              enter_long = true;
              exit_short = true;
            }
            EntropySignal::Down => {
              enter_short = true;
              exit_long = true;
            }
            _ => {}
          }
        }
      }
      None => match signal {
        EntropySignal::Up => {
          enter_long = true;
          exit_short = true;
        }
        EntropySignal::Down => {
          enter_short = true;
          exit_long = true;
        }
        _ => {}
      },
    }

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
        self.cache.push(data);
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

#[test]
fn test_fft_extrap() -> anyhow::Result<()> {
  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2025, 1, 1, None, None, None);
  let timeframe = "1d";
  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 15;
  let dominant_freq_cutoff = 100;
  let extrapolate = 3;
  let bits = EntropyBits::Two.bits();
  let patterns = EntropyBits::Two.patterns();

  let mut win = 0;
  let mut loss = 0;
  let mut cum_pnl = 0.0;

  let mut pnl_series = vec![];
  for i in 0..btc_series.len() - period + 1 - patterns {
    let data_ref = btc_series.data();

    let trained = Dataset::new(data_ref[i..i + period + 1].to_vec()).y();
    assert_eq!(trained.len(), period + 1);
    let expected = Dataset::new(data_ref[i + period..i + period + patterns].to_vec()).y();
    assert_eq!(expected.len(), patterns);

    let in_sample = Dataset::new(data_ref[i..i + period].to_vec());
    let FFT { predicted, .. } =
      dft_extrapolate(in_sample, dominant_freq_cutoff, extrapolate, true)?;
    let predicted = predicted.unwrap();
    let predicted_delta = predicted.y().last().unwrap() - predicted.y().first().unwrap();
    let extrap_up = predicted_delta > 0.0;

    let mut b11 = trained.clone();
    let mut b00 = trained.clone();
    let mut b10 = trained.clone();
    let mut b01 = trained.clone();

    // trained[0] = 1.0, b11[1] = 2.0, b11[0] = 3.0
    b11[1] = trained[0] + 1.0;
    b11[0] = b11[1] + 1.0;

    b00[1] = trained[0] - 1.0;
    b00[0] = b00[1] - 1.0;

    b10[1] = trained[0] + 1.0;
    b10[0] = b10[1] - 1.0;

    b01[1] = trained[0] - 1.0;
    b01[0] = b01[1] + 1.0;

    let length = period + 2;
    let entropy_b11 = shannon_entropy(&b11, length, patterns);
    let entropy_b00 = shannon_entropy(&b00, length, patterns);
    let entropy_b10 = shannon_entropy(&b10, length, patterns);
    let entropy_b01 = shannon_entropy(&b01, length, patterns);

    let last_index = expected.len() - 1;
    let p0 = expected[last_index];
    let p2 = expected[last_index - bits];

    let max = entropy_b11
      .max(entropy_b00)
      .max(entropy_b10)
      .max(entropy_b01);

    if max == entropy_b11 && p2 > p0 && extrap_up {
      win += 1;
      cum_pnl += p2 - p0;
    } else if max == entropy_b00 && p2 < p0 && !extrap_up {
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
  }

  println!(
    "trades: {}, win rate: {}%, profit: ${}",
    win + loss,
    trunc!(win as f64 / (win + loss) as f64 * 100.0, 3),
    trunc!(cum_pnl, 2)
  );

  Ok(())
}

#[test]
fn test_fft_and_entropy() -> anyhow::Result<()> {
  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2025, 1, 1, None, None, None);
  let timeframe = "1d";
  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 15;
  let dominant_freq_cutoff = 100;
  let bits = EntropyBits::Two.bits();
  let patterns = EntropyBits::Two.patterns();

  let FFT {
    trained, filtered, ..
  } = fft(btc_series, dominant_freq_cutoff)?;
  let filtered = filtered.unwrap();

  let trained_entropy = shannon_entropy(trained.y().as_slice(), period, patterns);
  let filtered_entropy = shannon_entropy(filtered.y().as_slice(), period, patterns);
  println!(
    "trained entropy: {}/{}",
    trunc!(trained_entropy, 3),
    patterns
  );
  println!(
    "filtered entropy: {}/{}",
    trunc!(filtered_entropy, 3),
    patterns
  );

  let mut win = 0;
  let mut loss = 0;
  let mut cum_pnl = 0.0;

  let mut entropies = vec![];
  let mut pnl_series = vec![];
  for i in 0..filtered.y().len() - period + 1 - patterns {
    // period is used to calc entropy, and next "patterns" are used to check if the entropy prediction is correct
    let series = filtered.y()[i..i + period + patterns].to_vec();

    let trained = filtered.y()[i..i + period].to_vec();
    assert_eq!(trained.len(), period);
    let expected = filtered.y()[i + period..i + period + patterns].to_vec();
    assert_eq!(expected.len(), patterns);

    let mut b11 = trained.clone();
    let mut b00 = trained.clone();
    let mut b10 = trained.clone();
    let mut b01 = trained.clone();

    b11[1] = trained[0] + 1.0;
    b11[0] = b11[1] + 1.0;

    b00[1] = trained[0] - 1.0;
    b00[0] = b00[1] - 1.0;

    b10[1] = trained[0] + 1.0;
    b10[0] = b10[1] - 1.0;

    b01[1] = trained[0] - 1.0;
    b01[0] = b01[1] + 1.0;

    let length = period + 1;
    let entropy_b11 = shannon_entropy(&b11, length, patterns);
    let entropy_b00 = shannon_entropy(&b00, length, patterns);
    let entropy_b10 = shannon_entropy(&b10, length, patterns);
    let entropy_b01 = shannon_entropy(&b01, length, patterns);

    let last_index = expected.len() - 1;
    let p0 = expected[last_index];
    let p2 = expected[last_index - bits];

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
  println!(
    "average filtered entropy for period {}: {}/{}",
    period,
    trunc!(avg_entropy, 3),
    patterns
  );

  println!(
    "trades: {}, win rate: {}%, profit: ${}",
    win + loss,
    trunc!(win as f64 / (win + loss) as f64 * 100.0, 3),
    trunc!(cum_pnl, 2)
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
    "btc_fft_entropy_pnl.png",
    "BTC Entropy",
    "$ PnL",
    "Time",
    Some(false),
  )?;

  Plot::plot_without_legend(
    vec![trained.0, filtered.0],
    "btc_fft_entropy.png",
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
  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2020, 1, 1, None, None, None);
  let timeframe = "1h";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 15;
  let bits = EntropyBits::One.bits();
  let patterns = EntropyBits::One.patterns();

  let mut win = 0;
  let mut loss = 0;
  let mut cum_pnl = 0.0;

  let mut entropies = vec![];
  let mut pnl_series = vec![];
  for i in 0..btc_series.y().len() - period + 1 - patterns {
    let series = btc_series.y()[i..i + period + patterns].to_vec();

    let trained = btc_series.y()[i..i + period].to_vec();
    assert_eq!(trained.len(), period);
    let expected = btc_series.y()[i + period..i + period + patterns].to_vec();
    assert_eq!(expected.len(), patterns);

    let mut b1 = trained.clone();
    let mut b0 = trained.clone();

    b1[0] = trained[0] + 1.0;
    b0[0] = trained[0] - 1.0;

    let length = period + 1;
    let entropy_b1 = shannon_entropy(&b1, length, patterns);
    let entropy_b0 = shannon_entropy(&b0, length, patterns);

    let last_index = expected.len() - 1;
    let p0 = series[last_index];
    let p1 = series[last_index - bits];

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
    Some(false),
  )?;

  Ok(())
}

/// Uses future bars to confirm whether entropy prediction was correct.
/// This is not to be used directly in a backtest, since future data is impossible.
#[test]
fn entropy_two_step() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();
  let clock_start = Time::now();
  let start_time = Time::new(2020, 1, 1, None, None, None);
  let end_time = Time::new(2022, 1, 1, None, None, None);

  let timeframe = "1h";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 15;
  let bits = EntropyBits::Two.bits();
  let patterns = EntropyBits::Two.patterns();

  let mut win = 0;
  let mut loss = 0;
  let mut cum_pnl = 0.0;

  let mut entropies = vec![];
  let mut pnl_series = vec![];

  for i in 0..btc_series.y().len() - period + 1 - patterns {
    let trained = btc_series.y()[i..i + period].to_vec();
    assert_eq!(trained.len(), period);
    let expected = btc_series.y()[i + period..i + period + patterns].to_vec();
    assert_eq!(expected.len(), patterns);

    let mut b11 = trained.clone();
    let mut b00 = trained.clone();
    let mut b10 = trained.clone();
    let mut b01 = trained.clone();

    b11[1] = trained[0] + 1.0;
    b11[0] = b11[1] + 1.0;

    b00[1] = trained[0] - 1.0;
    b00[0] = b00[1] - 1.0;

    b10[1] = trained[0] + 1.0;
    b10[0] = b10[1] - 1.0;

    b01[1] = trained[0] - 1.0;
    b01[0] = b01[1] + 1.0;

    let length = period + 1;
    let entropy_b11 = shannon_entropy(&b11, length, patterns);
    let entropy_b00 = shannon_entropy(&b00, length, patterns);
    let entropy_b10 = shannon_entropy(&b10, length, patterns);
    let entropy_b01 = shannon_entropy(&b01, length, patterns);

    let last_index = expected.len() - 1;
    let p0 = expected[last_index];
    let p2 = expected[last_index - bits];

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
    entropies.push(shannon_entropy(trained.as_slice(), period, patterns));
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
    Some(false),
  )?;

  Ok(())
}

#[test]
fn entropy_three_step() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();
  let clock_start = Time::now();
  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2025, 1, 1, None, None, None);
  let timeframe = "1h";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 15;
  let bits = EntropyBits::Three.bits();
  let patterns = EntropyBits::Three.patterns();

  let mut win = 0;
  let mut loss = 0;
  let mut cum_pnl = 0.0;

  let mut entropies = vec![];
  let mut pnl_series = vec![];
  for i in 0..btc_series.y().len() - period + 1 - patterns {
    let series = btc_series.y()[i..i + period + patterns].to_vec();

    let trained = btc_series.y()[i..i + period].to_vec();
    assert_eq!(trained.len(), period);
    let expected = btc_series.y()[i + period..i + period + patterns].to_vec();
    assert_eq!(expected.len(), patterns);

    let mut b111 = trained.clone();
    let mut b000 = trained.clone();
    let mut b110 = trained.clone();
    let mut b011 = trained.clone();
    let mut b101 = trained.clone();
    let mut b010 = trained.clone();
    let mut b100 = trained.clone();
    let mut b001 = trained.clone();

    b111[2] = trained[0] + 1.0;
    b111[1] = b111[2] + 1.0;
    b111[0] = b111[1] + 1.0;

    b000[2] = trained[0] - 1.0;
    b000[1] = b000[2] - 1.0;
    b000[0] = b000[1] - 1.0;

    b110[2] = trained[0] + 1.0;
    b110[1] = b110[2] + 1.0;
    b110[0] = b110[1] - 1.0;

    b011[2] = trained[0] - 1.0;
    b011[1] = b011[2] + 1.0;
    b011[0] = b011[1] + 1.0;

    b101[2] = trained[0] + 1.0;
    b101[1] = b101[2] - 1.0;
    b101[0] = b101[1] + 1.0;

    b010[2] = trained[0] - 1.0;
    b010[1] = b010[2] + 1.0;
    b010[0] = b010[1] - 1.0;

    b100[2] = trained[0] + 1.0;
    b100[1] = b100[2] - 1.0;
    b100[0] = b100[1] - 1.0;

    b001[2] = trained[0] - 1.0;
    b001[1] = b001[2] - 1.0;
    b001[0] = b001[1] + 1.0;

    let length = period + 1;
    let entropy_b111 = shannon_entropy(&b111, length, patterns);
    let entropy_b000 = shannon_entropy(&b000, length, patterns);
    let entropy_b110 = shannon_entropy(&b110, length, patterns);
    let entropy_b011 = shannon_entropy(&b011, length, patterns);
    let entropy_b101 = shannon_entropy(&b101, length, patterns);
    let entropy_b010 = shannon_entropy(&b010, length, patterns);
    let entropy_b100 = shannon_entropy(&b100, length, patterns);
    let entropy_b001 = shannon_entropy(&b001, length, patterns);

    let last_index = expected.len() - 1;
    let p0 = expected[last_index];
    let p3 = expected[last_index - bits];

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
    Some(false),
  )?;

  Ok(())
}

#[test]
fn entropy_zscore() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();
  let clock_start = Time::now();
  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2020, 1, 1, None, None, None);
  let timeframe = "1h";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 15;
  let patterns = 3;

  let mut win = 0;
  let mut loss = 0;
  let mut cum_pnl = 0.0;

  let mut entropies = vec![];
  let mut pnl_series = vec![];
  for i in 0..btc_series.y().len() - period + 1 {
    let series = btc_series.y()[i..i + period].to_vec();

    let last_index = series.len() - 1;

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

  let entropy_zscores: Vec<f64> = entropies
    .windows(period)
    .flat_map(|window| zscore(window, period))
    .collect();

  let zscore_data = Dataset::from(entropy_zscores.as_slice());

  if let Ok(halflife) = half_life(&entropies) {
    println!("halflife: {}", halflife);
  }

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

  Plot::plot(
    vec![Series {
      data: zscore_data.0,
      label: "Entropy Z-Score".to_string(),
    }],
    "btc_entropy_zscore.png",
    "BTC Entropy",
    "Z-Score",
    "Time",
    Some(false),
  )?;

  Ok(())
}

// ==========================================================================================
//                                 1d Backtest
// ==========================================================================================

#[test]
fn optimize_entropy_1d_backtest() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let fee = 0.25;
  let slippage = 0.0;
  let stop_loss = None;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = true;

  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2025, 1, 1, None, None, None);
  let timeframe = "1d";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let bits = EntropyBits::Two;

  let period_range = bits.patterns()..150;
  let zscore_range = [
    None,
    Some(1.0),
    Some(1.25),
    Some(1.5),
    Some(1.75),
    Some(2.0),
    Some(2.25),
    Some(2.5),
    Some(2.75),
    Some(3.0),
  ];

  let start = Time::now();
  let mut summaries: Vec<(usize, Option<f64>, Summary)> = period_range
    .into_par_iter()
    .flat_map(|period| {
      let summaries: Vec<(usize, Option<f64>, Summary)> = zscore_range
        .into_par_iter()
        .flat_map(|zscore| {
          let strat = EntropyBacktest::new(period, bits, zscore, ticker.clone(), stop_loss);
          let mut backtest = Backtest::builder(strat)
            .fee(fee)
            .slippage(slippage)
            .bet(bet)
            .leverage(leverage)
            .short_selling(short_selling);

          backtest
            .series
            .insert(ticker.clone(), series.data().clone());

          Result::<_, anyhow::Error>::Ok((period, zscore, backtest.backtest()?))
        })
        .collect();
      Result::<_, anyhow::Error>::Ok(summaries)
    })
    .flatten()
    .collect();
  println!(
    "optimized backtest in {}s",
    Time::now().to_unix() - start.to_unix()
  );

  // top 3 roi
  {
    summaries.sort_by(|(_, _, a), (_, _, b)| {
      b.pct_roi(&ticker)
        .partial_cmp(&a.pct_roi(&ticker))
        .unwrap_or(Ordering::Equal)
    });
    let top_3 = summaries.iter().take(3).collect::<Vec<_>>();

    println!("--- Top by ROI ---");
    for (period, zscore, summary) in top_3 {
      println!(
        "period: {}, zscore: {:?}, roi: {}%, sharpe: {}, dd: {}%",
        period,
        zscore,
        summary.pct_roi(&ticker),
        summary.sharpe_ratio(&ticker),
        summary.max_drawdown(&ticker)
      );
    }
  }

  // top 3 by sharpe ratio
  {
    summaries.sort_by(|(_, _, a), (_, _, b)| {
      b.sharpe_ratio(&ticker)
        .partial_cmp(&a.sharpe_ratio(&ticker))
        .unwrap_or(Ordering::Equal)
    });
    let top_3 = summaries.iter().take(3).collect::<Vec<_>>();

    println!("--- Top by Sharpe ---");
    for (period, zscore, summary) in top_3 {
      println!(
        "period: {}, zscore: {:?}, roi: {}%, sharpe: {}, dd: {}%",
        period,
        zscore,
        summary.pct_roi(&ticker),
        summary.sharpe_ratio(&ticker),
        summary.max_drawdown(&ticker)
      );
    }
  }

  // top 3 by drawdown
  {
    summaries.sort_by(|(_, _, a), (_, _, b)| {
      b.max_drawdown(&ticker)
        .partial_cmp(&a.max_drawdown(&ticker))
        .unwrap_or(Ordering::Equal)
    });
    let top_3 = summaries.iter().take(3).collect::<Vec<_>>();

    println!("--- Top by Drawdown ---");
    for (period, zscore, summary) in top_3 {
      println!(
        "period: {}, zscore: {:?}, roi: {}%, sharpe: {}, dd: {}%",
        period,
        zscore,
        summary.pct_roi(&ticker),
        summary.sharpe_ratio(&ticker),
        summary.max_drawdown(&ticker)
      );
    }
  }

  Ok(())
}

#[test]
fn entropy_1d_backtest() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let fee = 0.05;
  let slippage = 0.0;
  let stop_loss = None;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = true;

  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2025, 1, 1, None, None, None);
  let timeframe = "1d";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let bits = EntropyBits::Two;
  let period = 15;
  let zscore = None; //Some(2.5);

  let strat = EntropyBacktest::new(period, bits, zscore, ticker.clone(), stop_loss);
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

// ==========================================================================================
//                                 1h Backtest
// ==========================================================================================

#[test]
fn optimize_entropy_1h_backtest() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let fee = 0.25;
  let slippage = 0.0;
  let stop_loss = None;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = true;

  let start_time = Time::new(2019, 1, 1, None, None, None);
  let end_time = Time::new(2021, 1, 1, None, None, None);
  let timeframe = "1h";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let bits = EntropyBits::Two;

  let mut summaries: Vec<(usize, Summary)> = (bits.patterns()..150)
    .into_par_iter()
    .flat_map(|period| {
      let strat = EntropyBacktest::new(period, bits, None, ticker.clone(), stop_loss);
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

  // top 3 roi
  {
    summaries.sort_by(|(_, a), (_, b)| {
      b.pct_roi(&ticker)
        .partial_cmp(&a.pct_roi(&ticker))
        .unwrap_or(Ordering::Equal)
    });
    let top_3 = summaries.iter().take(3).collect::<Vec<_>>();

    println!("--- Top by ROI ---");
    for (period, summary) in top_3 {
      println!(
        "period: {}, roi: {}%, sharpe: {}, dd: {}%",
        period,
        summary.pct_roi(&ticker),
        summary.sharpe_ratio(&ticker),
        summary.max_drawdown(&ticker)
      );
    }
  }

  // top 3 by sharpe ratio
  {
    summaries.sort_by(|(_, a), (_, b)| {
      b.sharpe_ratio(&ticker)
        .partial_cmp(&a.sharpe_ratio(&ticker))
        .unwrap_or(Ordering::Equal)
    });
    let top_3 = summaries.iter().take(3).collect::<Vec<_>>();

    println!("--- Top by Sharpe ---");
    for (period, summary) in top_3 {
      println!(
        "period: {}, roi: {}%, sharpe: {}, dd: {}%",
        period,
        summary.pct_roi(&ticker),
        summary.sharpe_ratio(&ticker),
        summary.max_drawdown(&ticker)
      );
    }
  }

  // top 3 by drawdown
  {
    summaries.sort_by(|(_, a), (_, b)| {
      b.max_drawdown(&ticker)
        .partial_cmp(&a.max_drawdown(&ticker))
        .unwrap_or(Ordering::Equal)
    });
    let top_3 = summaries.iter().take(3).collect::<Vec<_>>();

    println!("--- Top by Drawdown ---");
    for (period, summary) in top_3 {
      println!(
        "period: {}, roi: {}%, sharpe: {}, dd: {}%",
        period,
        summary.pct_roi(&ticker),
        summary.sharpe_ratio(&ticker),
        summary.max_drawdown(&ticker)
      );
    }
  }

  Ok(())
}

#[test]
fn entropy_1h_backtest() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let fee = 0.0;
  let slippage = 0.0;
  let stop_loss = None;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = true;

  let start_time = Time::new(2018, 1, 1, None, None, None);
  let end_time = Time::new(2020, 1, 1, None, None, None);
  let timeframe = "1h";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 15;
  let bits = EntropyBits::Two;
  let zscore = None;

  let strat = EntropyBacktest::new(period, bits, zscore, ticker.clone(), stop_loss);
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

// #[test]
// fn shit_test() -> anyhow::Result<()> {
//   let start_time = Time::new(2017, 1, 1, None, None, None);
//   let end_time = Time::new(2025, 1, 1, None, None, None);
//   let timeframe = "1d";
//
//   let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
//   let ticker = "BTC".to_string();
//   let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;
//
//   let period = 15;
//
//   let capacity = period;
//   let mut cache = RingBuffer::new(capacity, ticker);
//
//   // First method is the backtest strategy
//   let mut last_seen = None;
//   let mut second_last_seen = None;
//   let mut first_method: Vec<(Vec<f64>, EntropySignal)> = vec![];
//   for (i, data) in btc_series.data().clone().into_iter().enumerate() {
//     cache.push(data);
//     if cache.vec.len() < capacity {
//       continue;
//     }
//     let series = Dataset::new(cache.vec());
//
//     if last_seen.is_some() {
//       if second_last_seen.is_none() {
//         println!("#1 second seen at {}: {:?}", i, series.y());
//       }
//       second_last_seen = last_seen.clone();
//     }
//
//     if last_seen.is_none() {
//       println!("#1 first seen at {}: {:?}", i, series.y());
//     }
//
//     last_seen = Some(series.y());
//
//     let signal = two_step_entropy_signal(series.cloned(), period)?;
//     let y = series.y();
//     first_method.push((y, signal));
//   }
//
//   if let Some(last_seen) = last_seen {
//     println!("#1 last seen: {:?}", last_seen);
//   }
//   if let Some(second_last_seen) = second_last_seen {
//     println!("#1 second_last_seen: {:?}", second_last_seen);
//   }
//
//   // Second method is the isolated entropy test
//   let second_method: Vec<(Vec<f64>, EntropySignal)> =
//     _two_step_entropy_signals(btc_series, period)?;
//
//   // deep equality check first_method and second_method
//   let mut does_match = true;
//
//   if first_method.len() != second_method.len() {
//     println!(
//       "result lengths do not match, {} != {}",
//       first_method.len(),
//       second_method.len()
//     );
//     does_match = false;
//   }
//
//   if does_match {
//     let checks: Vec<bool> = (0..first_method.len())
//       .map(|i| {
//         let mut does_match = true;
//
//         let first: &(Vec<f64>, EntropySignal) = &first_method[i];
//         let (first_data, first_signal) = first;
//         let second: &(Vec<f64>, EntropySignal) = &second_method[i];
//         let (second_data, second_signal) = second;
//
//         if first_data.len() != second_data.len() {
//           println!(
//             "lengths[{}], {} != {}",
//             i,
//             first_data.len(),
//             second_data.len()
//           );
//           does_match = false;
//         }
//
//         if does_match {
//           // check if first_signal and second_signal match
//           if first_signal != second_signal {
//             println!("signals[{}], {:?} != {:?}", i, first_signal, second_signal);
//             does_match = false;
//           }
//         }
//
//         if does_match {
//           for (first, second) in first_data.iter().zip(second_data.iter()) {
//             if first != second {
//               println!("y[{}]", i);
//               does_match = false;
//               break;
//             }
//           }
//         }
//         does_match
//       })
//       .collect();
//
//     // if not all "checks" are true then set "does_match" to false
//     if checks.iter().any(|check| !check) {
//       does_match = false;
//     }
//   }
//   match does_match {
//     true => {
//       println!("results match");
//       Ok(())
//     }
//     false => Err(anyhow::anyhow!("results do not match")),
//   }
// }
