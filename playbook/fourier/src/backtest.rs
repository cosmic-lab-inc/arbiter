#![allow(unused_imports)]

use crate::trade::{Bet, Signal, SignalInfo};
use crate::{Backtest, Dataset, Strategy};
use log::warn;
use ndarray::AssignElem;
use nexus::*;
use rayon::prelude::*;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::ops::Neg;
use std::path::{Path, PathBuf};
use tradestats::kalman::*;
use tradestats::metrics::*;
use tradestats::utils::*;

#[derive(Debug, Clone)]
pub struct FourierBacktest {
  period: usize,
  dominant_freq_cutoff: usize,
  pub cache: RingBuffer<Data>,
  assets: Assets,
  pub stop_loss_pct: Option<f64>,
}

impl FourierBacktest {
  pub fn new(
    period: usize,
    dominant_freq_cutoff: usize,
    ticker: String,
    stop_loss_pct: Option<f64>,
  ) -> Self {
    Self {
      period,
      dominant_freq_cutoff,
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

    let FFT { filtered, .. } = fft(series, self.dominant_freq_cutoff)?;
    let sample = filtered.unwrap();

    // compute first and second derivative of "filtered" to get the momentum and acceleration
    let dy_1 = sample.derivative();
    let dy_2 = dy_1.derivative();

    let mom_slope = dy_1.slope();
    let accel_slope = dy_2.slope();

    if mom_slope > 0.0 && accel_slope > 0.0 {
      enter_long = true;
      exit_short = true;
    } else if mom_slope < 0.0 && accel_slope < 0.0 {
      enter_short = true;
      exit_long = true;
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

impl Strategy<Data> for FourierBacktest {
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
    "fourier".to_string()
  }
}

// ==========================================================================================
//                                 Fast Fourier Transform
// ==========================================================================================

#[test]
fn extrap_methods() -> anyhow::Result<()> {
  let start_time = Time::new(2019, 3, 1, None, None, None);
  let end_time = Time::new(2019, 4, 1, None, None, None);
  let timeframe = "1h";
  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let dominant_freq_cutoff = 25;
  let extrap_only = false;
  let extrapolate = 100;

  let (in_sample, out_sample) = btc_series.enumerate_map().sample(extrapolate);

  let FFT { predicted, .. } = dft_extrapolate(
    in_sample.clone(),
    dominant_freq_cutoff,
    extrapolate,
    extrap_only,
  )?;

  // let quad_lsr = quad_lsr_extrap(in_sample.clone(), extrapolate, extrap_only);
  // let cubic_lsr = cubic_lsr_extrap(in_sample.clone(), extrapolate, extrap_only);
  // let varpro = varpro_lsr_extrap(in_sample.clone(), extrapolate, extrap_only);

  Plot::plot(
    vec![
      Series {
        data: in_sample.0,
        label: "In Sample".to_string(),
      },
      Series {
        data: out_sample.0,
        label: "Out Sample".to_string(),
      },
      Series {
        data: predicted.unwrap().0,
        label: "DFT".to_string(),
      },
      // Series {
      //   data: cubic_lsr.0,
      //   label: "Quadratic LSR".to_string(),
      // },
      // Series {
      //   data: quad_lsr.0,
      //   label: "Cubic LSR".to_string(),
      // },
      // Series {
      //   data: varpro.0,
      //   label: "VarPro LSR".to_string(),
      // },
    ],
    "btc_extrap_methods.png",
    "BTC Extrapolation",
    "Price",
    "Time",
    Some(false),
  )?;

  Ok(())
}

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
  let extrapolate = 2;

  let mut win = 0;
  let mut loss = 0;
  let mut cum_pnl = 0.0;

  for i in 0..btc_series.len() - period + 1 - extrapolate {
    let period_series = Dataset::new(btc_series.data()[i..i + period + extrapolate].to_vec());

    let (in_sample, out_sample) = period_series.sample(extrapolate);
    assert_eq!(in_sample.len(), period);
    assert_eq!(out_sample.len(), extrapolate);

    let FFT { filtered, .. } = fft(in_sample, dominant_freq_cutoff)?;
    let filtered_period_series = filtered.unwrap();

    let FFT { predicted, .. } = dft_extrapolate(
      filtered_period_series,
      dominant_freq_cutoff,
      extrapolate,
      true,
    )?;
    let predicted = predicted.unwrap();
    let predicted_delta = predicted.y().last().unwrap() - predicted.y().first().unwrap();
    let extrap_up = predicted_delta > 0.0;

    let actual_delta = out_sample.0.last().unwrap().y - out_sample.0.first().unwrap().y;

    if extrap_up && actual_delta > 0.0 {
      win += 1;
      cum_pnl += actual_delta;
    } else if !extrap_up && actual_delta < 0.0 {
      win += 1;
      cum_pnl += actual_delta.abs();
    } else if extrap_up && actual_delta < 0.0 {
      loss += 1;
      cum_pnl -= actual_delta;
    } else if !extrap_up && actual_delta > 0.0 {
      loss += 1;
      cum_pnl -= actual_delta.neg();
    }
  }

  println!(
    "trades: {}, win rate: {}%, profit: ${}",
    win + loss,
    trunc!(win as f64 / (win + loss) as f64 * 100.0, 3),
    trunc!(cum_pnl, 2)
  );

  Ok(())
}

// ==========================================================================================
//                                 1d Backtest
// ==========================================================================================

#[test]
fn optimize_fft_1d_backtest() -> anyhow::Result<()> {
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

  let bits = EntropyBits::Two;

  let period_range = bits.patterns()..200;
  let freq_cutoff_range = 10..100;

  let summaries: Vec<(usize, Summary)> = period_range
    .into_par_iter()
    .flat_map(|period| {
      let summaries: Vec<(usize, Summary)> = freq_cutoff_range
        .clone()
        .into_par_iter()
        .flat_map(|dominant_freq_cutoff| {
          let strat = FourierBacktest::new(period, dominant_freq_cutoff, ticker.clone(), stop_loss);
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
      Result::<_, anyhow::Error>::Ok(summaries)
    })
    .flatten()
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
fn fft_1d_backtest() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let fee = 0.0;
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

  let period = 100;
  let dominant_freq_cutoff = 50;

  let strat = FourierBacktest::new(period, dominant_freq_cutoff, ticker.clone(), stop_loss);
  let mut backtest = Backtest::builder(strat)
    .fee(fee)
    .slippage(slippage)
    .bet(bet)
    .leverage(leverage)
    .short_selling(short_selling);

  backtest
    .series
    .insert(ticker.clone(), series.data().clone());

  backtest.execute("Fourier Backtest", timeframe)?;

  Ok(())
}
