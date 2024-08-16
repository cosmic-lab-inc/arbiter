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

    // let FFT {
    //   original: _original,
    //   filtered,
    // } = fft(series.clone(), 100)?;
    // let entropy = shannon_entropy(filtered.y().as_slice(), self.period, self.patterns);
    // if entropy == 0.0 {
    //   return Err(anyhow::anyhow!("Entropy is zero"));
    // }

    let signal = shannon_entropy_signal(series, self.period, self.patterns)?;

    let mut enter_long = false;
    let mut exit_long = false;
    let mut enter_short = false;
    let mut exit_short = false;

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

#[test]
fn test_fft_extrapolate() -> anyhow::Result<()> {
  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2020, 6, 1, None, None, None);
  let timeframe = "1d";
  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let dominant_freq_cutoff = 50;
  let extrapolate = 100;

  let data = btc_series.enumerate_map();
  let (training, expected) = data.training_data(extrapolate);

  let FFT {
    predicted,
    filtered,
    ..
  } = fft_extrapolate(training.cloned(), dominant_freq_cutoff, extrapolate)?;

  Plot::plot(
    vec![
      Series {
        data: training.cloned().0,
        label: "In Sample".to_string(),
      },
      Series {
        data: predicted.unwrap().0,
        label: "Predicted".to_string(),
      },
      Series {
        data: expected.cloned().0,
        label: "Out Sample".to_string(),
      },
      // Series {
      //   data: filtered.0,
      //   label: "Filtered IFFT".to_string(),
      // },
    ],
    "btc_fft_extrap.png",
    &format!("{} DFT", ticker),
    "Price",
    "Time",
    Some(false),
  )?;

  Ok(())
}

#[test]
fn test_fft() -> anyhow::Result<()> {
  let start_time = Time::new(2020, 7, 1, None, None, None);
  let end_time = Time::new(2022, 7, 1, None, None, None);
  let timeframe = "1h";
  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 15;
  let patterns = 3;
  let dominant_freq_cutoff = 100;

  let FFT {
    trained, filtered, ..
  } = fft(btc_series, dominant_freq_cutoff)?;

  let trained_entropy = shannon_entropy(trained.y().as_slice(), period + 1, patterns);
  let filtered_entropy = shannon_entropy(filtered.y().as_slice(), period + 1, patterns);

  println!(
    "original entropy: {}/{}",
    trunc!(trained_entropy, 3),
    patterns
  );
  println!(
    "filtered entropy: {}/{}",
    trunc!(filtered_entropy, 3),
    patterns
  );

  Plot::plot_without_legend(
    vec![trained.0, filtered.0],
    "btc_ifft.png",
    &format!("{} Inverse FFT", ticker),
    "Price",
    "Time",
  )?;

  Ok(())
}

#[test]
fn test_fft_and_entropy() -> anyhow::Result<()> {
  let start_time = Time::new(2020, 6, 1, None, None, None);
  let end_time = Time::new(2020, 7, 1, None, None, None);
  let timeframe = "1m";
  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 1000;
  let patterns = 3;
  let dominant_freq_cutoff = 100;

  let FFT {
    trained, filtered, ..
  } = fft(btc_series, dominant_freq_cutoff)?;

  let trained_entropy = shannon_entropy(trained.y().as_slice(), period + 1, patterns);
  let filtered_entropy = shannon_entropy(filtered.y().as_slice(), period + 1, patterns);

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

  Plot::plot_without_legend(
    vec![trained.0, filtered.0],
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
    None,
  )?;

  Ok(())
}

#[test]
fn entropy_two_step() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();
  let clock_start = Time::now();
  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2024, 7, 1, None, None, None);

  let timeframe = "1d";

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
  for i in 0..btc_series.y().len() - period {
    let series = btc_series.y()[i..i + period].to_vec();
    let last_index = series.len() - 1;

    let mut b11 = series[..last_index - 2].to_vec();
    let mut b00 = series[..last_index - 2].to_vec();
    let mut b10 = series[..last_index - 2].to_vec();
    let mut b01 = series[..last_index - 2].to_vec();

    // let mut b11 = series.clone();
    // let mut b00 = series.clone();
    // let mut b10 = series.clone();
    // let mut b01 = series.clone();

    b11[1] = series[0] + 1.0;
    b11[0] = b11[1] + 1.0;

    b00[1] = series[0] - 1.0;
    b00[0] = b00[1] - 1.0;

    b10[1] = series[0] + 1.0;
    b10[0] = b10[1] - 1.0;

    b01[1] = series[0] - 1.0;
    b01[0] = b01[1] + 1.0;

    // let entropy_b11 = shannon_entropy(&b11, period + 1, patterns);
    // let entropy_b00 = shannon_entropy(&b00, period + 1, patterns);
    // let entropy_b10 = shannon_entropy(&b10, period + 1, patterns);
    // let entropy_b01 = shannon_entropy(&b01, period + 1, patterns);

    let entropy_b11 = shannon_entropy(&b11, period - 2, patterns);
    let entropy_b00 = shannon_entropy(&b00, period - 2, patterns);
    let entropy_b10 = shannon_entropy(&b10, period - 2, patterns);
    let entropy_b01 = shannon_entropy(&b01, period - 2, patterns);

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
    None,
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
    None,
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

  let fee = 0.0;
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

  let period = 15;
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
