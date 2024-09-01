#![allow(unused_imports)]
#![allow(dead_code)]

use log::debug;
use nexus::*;
use std::cmp::Ordering;
use std::collections::HashMap;
use tradestats::metrics::{pearson_correlation_coefficient, rolling_correlation, rolling_zscore};

#[derive(Debug, Clone)]
struct LastSignal {
  pub bars_since: usize,
  pub signal: EntropySignal,
  pub price: f64,
}

#[derive(Debug, Clone)]
pub struct EntropyBacktest {
  period: usize,
  bits: EntropyBits,
  entropy_zscore_cutoff: Option<f64>,
  pub cache: RingBuffer<Data>,
  assets: Positions,
  pub stop_loss_pct: Option<f64>,
  last_signal: Option<LastSignal>,

  e_cache: RingBuffer<f64>,
  ez_cache: RingBuffer<f64>,
  pz_cache: RingBuffer<f64>,
  ema_cache: RingBuffer<f64>,

  longs_won: usize,
  longs_lost: usize,
  longs_pnl: f64,
  shorts_won: usize,
  shorts_lost: usize,
  shorts_pnl: f64,
}

impl Strategy<Data> for EntropyBacktest {
  fn process_data(
    &mut self,
    data: Data,
    ticker: Option<String>,
    assets: &Positions,
    active_trades: &ActiveTrades,
  ) -> anyhow::Result<Vec<Signal>> {
    if let Some(ticker) = ticker.clone() {
      if ticker == self.cache.id {
        self.cache.push(data);
      }
    }
    if let Some(LastSignal {
      bars_since,
      signal,
      price,
    }) = self.last_signal
    {
      self.last_signal = Some(LastSignal {
        bars_since: bars_since + 1,
        signal,
        price,
      });
    }
    self.assets = assets.clone();
    self.signal(ticker, active_trades)
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

impl EntropyBacktest {
  pub fn new(
    period: usize,
    bits: EntropyBits,
    entropy_zscore_cutoff: Option<f64>,
    ticker: String,
    stop_loss_pct: Option<f64>,
  ) -> Self {
    Self {
      period,
      bits,
      entropy_zscore_cutoff,
      cache: RingBuffer::new(period, ticker),
      assets: Positions::default(),
      stop_loss_pct,
      last_signal: None,

      e_cache: RingBuffer::new(period, String::new()),
      ez_cache: RingBuffer::new(period, String::new()),
      pz_cache: RingBuffer::new(period, String::new()),
      ema_cache: RingBuffer::new(period, String::new()),

      longs_won: 0,
      longs_pnl: 0.0,
      longs_lost: 0,
      shorts_won: 0,
      shorts_lost: 0,
      shorts_pnl: 0.0,
    }
  }

  fn binary_signals(&mut self) -> anyhow::Result<Signals> {
    let mut enter_long = false;
    let mut exit_long = false;
    let mut enter_short = false;
    let mut exit_short = false;

    let series = Dataset::new(self.cache.vec());
    let signal = n_bit_entropy!(self.bits.bits(), self.period, series.y())?;

    match self.entropy_zscore_cutoff {
      Some(cutoff) => {
        let z = zscore(&series.y(), self.period)?;
        if z.abs() > cutoff {
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

  fn hold_for_bits_signals(&mut self) -> anyhow::Result<Signals> {
    let mut enter_long = false;
    let mut exit_long = false;
    let mut enter_short = false;
    let mut exit_short = false;

    let series = Dataset::new(self.cache.vec());

    let latest_price = *series.y().last().unwrap();
    let mut new_last_signal = self.last_signal.clone();
    // exit position created by last_signal if needed
    if let Some(LastSignal {
      bars_since,
      signal,
      price,
    }) = new_last_signal
    {
      if bars_since > self.bits.bits() {
        match signal {
          EntropySignal::Up => {
            if latest_price > price {
              self.longs_won += 1;
            } else {
              self.longs_lost += 1;
            }
            self.longs_pnl += latest_price - price;
            exit_long = true;
            new_last_signal = None;
          }
          EntropySignal::Down => {
            if latest_price < price {
              self.shorts_won += 1;
            } else {
              self.shorts_lost += 1;
            }
            self.shorts_pnl += price - latest_price;
            exit_short = true;
            new_last_signal = None;
          }
          _ => {}
        }
      }
    }
    // only trade if no active position
    if new_last_signal.is_none() {
      let signal = n_bit_entropy!(self.bits.bits(), self.period, series.y())?;
      match signal {
        EntropySignal::Up => {
          enter_long = true;
          new_last_signal = Some(LastSignal {
            bars_since: 0,
            signal: EntropySignal::Up,
            price: latest_price,
          });
        }
        EntropySignal::Down => {
          enter_short = true;
          new_last_signal = Some(LastSignal {
            bars_since: 0,
            signal: EntropySignal::Down,
            price: latest_price,
          });
        }
        _ => {}
      }
    }
    self.last_signal = new_last_signal;
    debug!(
      "longs {}/{}, ${}, shorts: {}/{}, ${}",
      self.longs_won,
      self.longs_won + self.longs_lost,
      trunc!(self.longs_pnl, 2),
      self.shorts_won,
      self.shorts_won + self.shorts_lost,
      trunc!(self.shorts_pnl, 2)
    );

    Ok(Signals {
      enter_long,
      exit_long,
      enter_short,
      exit_short,
    })
  }

  fn entropy_zscore_signals(&mut self) -> anyhow::Result<Signals> {
    let mut enter_long = false;
    let mut exit_long = false;
    let mut enter_short = false;
    let mut exit_short = false;

    let series = Dataset::new(self.cache.vec());

    //
    // Cache entropy and price z-scores
    //

    let closes = series.y();
    let e = shannon_entropy(closes.as_slice(), self.period + 1, self.bits.patterns());
    self.e_cache.push(e);
    let pz = zscore(closes.as_slice(), self.period)?;
    self.pz_cache.push(pz);

    let e_series = self.e_cache.vec();
    // not enough values to compute entropy z-score or price z-score
    if e_series.len() < self.period
      || self.pz_cache.vec().len() < self.period
      || self.ema_cache.vec().len() < self.period
    {
      return Ok(Signals {
        enter_long,
        exit_long,
        enter_short,
        exit_short,
      });
    }
    let ez = zscore(e_series.as_slice(), self.period)?;
    self.ez_cache.push(ez);
    // not enough entropy z-score values to compare to price z-score
    if self.ez_cache.vec().len() < self.period {
      return Ok(Signals {
        enter_long,
        exit_long,
        enter_short,
        exit_short,
      });
    }

    //
    // If sufficient data caches, then check for signal
    //

    let can_trade = ez.abs() > self.entropy_zscore_cutoff.unwrap_or(0.0);
    if can_trade {
      let latest_price = *series.y().last().unwrap();
      let mut new_last_signal = self.last_signal.clone();
      // exit position created by last_signal if needed
      if let Some(LastSignal {
        bars_since,
        signal,
        price,
      }) = new_last_signal
      {
        if bars_since > self.bits.bits() {
          match signal {
            EntropySignal::Up => {
              if latest_price > price {
                self.longs_won += 1;
              } else {
                self.longs_lost += 1;
              }
              self.longs_pnl += latest_price - price;
              exit_long = true;
              new_last_signal = None;
            }
            EntropySignal::Down => {
              if latest_price < price {
                self.shorts_won += 1;
              } else {
                self.shorts_lost += 1;
              }
              self.shorts_pnl += price - latest_price;
              exit_short = true;
              new_last_signal = None;
            }
            _ => {}
          }
        }
      }
      // only trade if no active position
      if new_last_signal.is_none() {
        let signal = n_bit_entropy!(self.bits.bits(), self.period, series.y())?;
        match signal {
          EntropySignal::Up => {
            enter_long = true;
            new_last_signal = Some(LastSignal {
              bars_since: 0,
              signal: EntropySignal::Up,
              price: latest_price,
            });
          }
          EntropySignal::Down => {
            enter_short = true;
            new_last_signal = Some(LastSignal {
              bars_since: 0,
              signal: EntropySignal::Down,
              price: latest_price,
            });
          }
          _ => {}
        }
      }
      self.last_signal = new_last_signal;
      debug!(
        "longs {}/{}, ${}, shorts: {}/{}, ${}",
        self.longs_won,
        self.longs_won + self.longs_lost,
        trunc!(self.longs_pnl, 2),
        self.shorts_won,
        self.shorts_won + self.shorts_lost,
        trunc!(self.shorts_pnl, 2)
      );
    }

    Ok(Signals {
      enter_long,
      exit_long,
      enter_short,
      exit_short,
    })
  }

  fn generate_signals(&mut self) -> anyhow::Result<Signals> {
    self.entropy_zscore_signals()
  }

  pub fn signal(
    &mut self,
    t: Option<String>,
    trades: &ActiveTrades,
  ) -> anyhow::Result<Vec<Signal>> {
    match t {
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

        let Data { x: time, y: price } = *self
          .cache
          .front()
          .ok_or(anyhow::anyhow!("No data in cache"))?;

        let mut signals: Vec<Signal> = vec![];

        let id = 0;

        let enter_long_key = Trade::build_key(&ticker, TradeAction::EnterLong, id);
        let active_long = trades.get(&enter_long_key);
        let mut has_long = active_long.is_some();

        let enter_short_key = Trade::build_key(&ticker, TradeAction::EnterShort, id);
        let active_short = trades.get(&enter_short_key);
        let mut has_short = active_short.is_some();

        let bet = Bet::Percent(100.0);

        if exit_short && has_short {
          let trade = Signal {
            id,
            price,
            date: Time::from_unix_ms(time),
            ticker: ticker.clone(),
            bet: None, // not needed, calculated in backtest using entry
            side: TradeAction::ExitShort,
          };
          signals.push(trade);
          has_short = false;
        }

        if exit_long && has_long {
          let trade = Signal {
            id,
            price,
            date: Time::from_unix_ms(time),
            ticker: ticker.clone(),
            bet: None,
            side: TradeAction::ExitLong,
          };
          signals.push(trade);
          has_long = false;
        }

        if enter_short && !has_short && !has_long {
          let trade = Signal {
            id,
            price,
            date: Time::from_unix_ms(time),
            ticker: ticker.clone(),
            bet: Some(bet),
            side: TradeAction::EnterShort,
          };
          signals.push(trade);
        }

        if enter_long && !has_short && !has_long {
          let trade = Signal {
            id,
            price,
            date: Time::from_unix_ms(time),
            ticker: ticker.clone(),
            bet: Some(bet),
            side: TradeAction::EnterLong,
          };
          signals.push(trade);
        }

        Ok(signals)
      }
    }
  }
}

// ==========================================================================================
//                                 1d Backtest
// ==========================================================================================

/// Just in case you decide to "parallelize" the optimization process to make it "faster".
/// par iter zscore = 108s
/// par iter both = 209s
/// par iter neither = 69s
#[test]
fn optimize_entropy_1d_backtest() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();
  init_logger();

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

  let period_range = 4..500;
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

  struct Params {
    pub summary: Summary,
    pub period: usize,
    pub zscore: Option<f64>,
    pub pct_roi: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
  }

  let timer = Timer::new();

  let mut summaries = vec![];
  for period in period_range {
    for zscore in zscore_range {
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

      let timer = Timer::new();
      let summary = backtest.backtest()?;
      let num_trades = summary
        .trades
        .get(&ticker)
        .map(|trades| trades.len())
        .unwrap_or(0);
      debug!("{} trades / {}ms", num_trades, timer.millis());

      let params = Params {
        period,
        zscore,
        pct_roi: summary.pct_roi(&ticker),
        sharpe_ratio: summary.sharpe_ratio(&ticker),
        max_drawdown: summary.max_drawdown(&ticker),
        summary,
      };
      summaries.push(params);
    }
  }
  println!("optimized backtest in {}s", timer.seconds());

  // top 3 roi
  {
    summaries.sort_by(|a, b| b.pct_roi.partial_cmp(&a.pct_roi).unwrap_or(Ordering::Equal));
    let top_3 = summaries.iter().take(3).collect::<Vec<_>>();

    println!("**Top by ROI**");
    for params in top_3 {
      println!(
        "period: {}, zscore: {:?}, roi: {}%, sharpe: {}, dd: {}%",
        params.period, params.zscore, params.pct_roi, params.sharpe_ratio, params.max_drawdown,
      );
    }
  }

  // top 3 by sharpe ratio
  {
    summaries.sort_by(|a, b| {
      b.sharpe_ratio
        .partial_cmp(&a.sharpe_ratio)
        .unwrap_or(Ordering::Equal)
    });
    let top_3 = summaries.iter().take(3).collect::<Vec<_>>();

    println!("**Top by Sharpe**");
    for params in top_3 {
      println!(
        "period: {}, zscore: {:?}, roi: {}%, sharpe: {}, dd: {}%",
        params.period, params.zscore, params.pct_roi, params.sharpe_ratio, params.max_drawdown,
      );
    }
  }

  // top 3 by drawdown
  {
    summaries.sort_by(|a, b| {
      b.max_drawdown
        .partial_cmp(&a.max_drawdown)
        .unwrap_or(Ordering::Equal)
    });
    let top_3 = summaries.iter().take(3).collect::<Vec<_>>();

    println!("**Top by Drawdown**");
    for params in top_3 {
      println!(
        "period: {}, zscore: {:?}, roi: {}%, sharpe: {}, dd: {}%",
        params.period, params.zscore, params.pct_roi, params.sharpe_ratio, params.max_drawdown,
      );
    }
  }

  Ok(())
}

#[test]
fn entropy_1d_backtest() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();
  init_logger();

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

  let bits = EntropyBits::Two;
  let period = 100;
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
  let short_selling = false;

  let start_time = Time::new(2019, 1, 1, None, None, None);
  let end_time = Time::new(2021, 1, 1, None, None, None);
  let timeframe = "1h";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let bits = EntropyBits::Two;

  let period_range = 10..100;
  let zscore_range = [
    None,
    // Some(1.0),
    // Some(1.25),
    // Some(1.5),
    // Some(1.75),
    // Some(2.0),
    // Some(2.25),
    // Some(2.5),
    // Some(2.75),
    // Some(3.0),
  ];

  struct Params {
    pub summary: Summary,
    pub period: usize,
    pub zscore: Option<f64>,
    pub pct_roi: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
  }

  let timer = Timer::new();

  let mut summaries = vec![];
  for period in period_range {
    for zscore in zscore_range {
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

      let timer = Timer::new();
      let summary = backtest.backtest()?;
      println!("{}-{:?}, {}ms", period, zscore, timer.millis());
      let params = Params {
        period,
        zscore,
        pct_roi: summary.pct_roi(&ticker),
        sharpe_ratio: summary.sharpe_ratio(&ticker),
        max_drawdown: summary.max_drawdown(&ticker),
        summary,
      };
      summaries.push(params);
    }
  }

  println!("optimized backtest in {}s", timer.seconds());

  // top 3 roi
  {
    summaries.sort_by(|a, b| b.pct_roi.partial_cmp(&a.pct_roi).unwrap_or(Ordering::Equal));
    let top_3 = summaries.iter().take(3).collect::<Vec<_>>();

    println!("**Top by ROI**");
    for params in top_3 {
      println!(
        "period: {}, zscore: {:?}, roi: {}%, sharpe: {}, dd: {}%",
        params.period, params.zscore, params.pct_roi, params.sharpe_ratio, params.max_drawdown,
      );
    }
  }

  // top 3 by sharpe ratio
  {
    summaries.sort_by(|a, b| {
      b.sharpe_ratio
        .partial_cmp(&a.sharpe_ratio)
        .unwrap_or(Ordering::Equal)
    });
    let top_3 = summaries.iter().take(3).collect::<Vec<_>>();

    println!("**Top by Sharpe**");
    for params in top_3 {
      println!(
        "period: {}, zscore: {:?}, roi: {}%, sharpe: {}, dd: {}%",
        params.period, params.zscore, params.pct_roi, params.sharpe_ratio, params.max_drawdown,
      );
    }
  }

  // top 3 by drawdown
  {
    summaries.sort_by(|a, b| {
      b.max_drawdown
        .partial_cmp(&a.max_drawdown)
        .unwrap_or(Ordering::Equal)
    });
    let top_3 = summaries.iter().take(3).collect::<Vec<_>>();

    println!("**Top by Drawdown**");
    for params in top_3 {
      println!(
        "period: {}, zscore: {:?}, roi: {}%, sharpe: {}, dd: {}%",
        params.period, params.zscore, params.pct_roi, params.sharpe_ratio, params.max_drawdown,
      );
    }
  }

  Ok(())
}

#[test]
fn entropy_1h_backtest() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();
  init_logger();

  let fee = 0.0;
  let slippage = 0.0;
  let stop_loss = None;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = false;

  let start_time = Time::new(2019, 1, 1, None, None, None);
  let end_time = Time::new(2024, 1, 1, None, None, None);
  let timeframe = "1h";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 100;
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
