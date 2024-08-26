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
  entropy_bits: EntropyBits,
  entropy_zscore_cutoff: Option<f64>,
  pub cache: RingBuffer<Data>,
  assets: Positions,
  pub stop_loss_pct: Option<f64>,
  last_signal: Option<LastSignal>,
  e_cache: RingBuffer<f64>,
  ez_cache: RingBuffer<f64>,
  pz_cache: RingBuffer<f64>,

  longs_won: usize,
  longs_lost: usize,
  longs_pnl: f64,
  shorts_won: usize,
  shorts_lost: usize,
  shorts_pnl: f64,
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
      cache: RingBuffer::new(period, ticker),
      assets: Positions::default(),
      stop_loss_pct,
      last_signal: None,
      e_cache: RingBuffer::new(period, String::new()),
      ez_cache: RingBuffer::new(period, String::new()),
      pz_cache: RingBuffer::new(period, String::new()),

      longs_won: 0,
      longs_pnl: 0.0,
      longs_lost: 0,
      shorts_won: 0,
      shorts_lost: 0,
      shorts_pnl: 0.0,
    }
  }

  fn generate_signals(&mut self) -> anyhow::Result<Signals> {
    let mut enter_long = false;
    let mut exit_long = false;
    let mut enter_short = false;
    let mut exit_short = false;

    let series = Dataset::new(self.cache.vec());

    // --- #1 METHOD ---
    // let latest_price = *series.y().last().unwrap();
    // let mut new_last_signal = self.last_signal.clone();
    // // exit position created by last_signal if needed
    // if let Some(LastSignal {
    //   bars_since,
    //   signal,
    //   price,
    // }) = new_last_signal
    // {
    //   if bars_since > self.entropy_bits.bits() {
    //     match signal {
    //       EntropySignal::Up => {
    //         if latest_price > price {
    //           self.longs_won += 1;
    //         } else {
    //           self.longs_lost += 1;
    //         }
    //         self.longs_pnl += latest_price - price;
    //         exit_long = true;
    //         new_last_signal = None;
    //       }
    //       EntropySignal::Down => {
    //         if latest_price < price {
    //           self.shorts_won += 1;
    //         } else {
    //           self.shorts_lost += 1;
    //         }
    //         self.shorts_pnl += price - latest_price;
    //         exit_short = true;
    //         new_last_signal = None;
    //       }
    //       _ => {}
    //     }
    //   }
    // }
    // // only trade if no active position
    // if new_last_signal.is_none() {
    //   let signal = match self.entropy_bits {
    //     EntropyBits::One => one_step_entropy_signal(series, self.period)?,
    //     EntropyBits::Two => two_step_entropy_signal(series, self.period)?,
    //     EntropyBits::Three => three_step_entropy_signal(series, self.period)?,
    //   };
    //   match signal {
    //     EntropySignal::Up => {
    //       enter_long = true;
    //       new_last_signal = Some(LastSignal {
    //         bars_since: 0,
    //         signal: EntropySignal::Up,
    //         price: latest_price,
    //       });
    //     }
    //     EntropySignal::Down => {
    //       enter_short = true;
    //       new_last_signal = Some(LastSignal {
    //         bars_since: 0,
    //         signal: EntropySignal::Down,
    //         price: latest_price,
    //       });
    //     }
    //     _ => {}
    //   }
    // }
    // self.last_signal = new_last_signal;
    // debug!(
    //   "longs {}/{}, ${}, shorts: {}/{}, ${}",
    //   self.longs_won,
    //   self.longs_won + self.longs_lost,
    //   trunc!(self.longs_pnl, 2),
    //   self.shorts_won,
    //   self.shorts_won + self.shorts_lost,
    //   trunc!(self.shorts_pnl, 2)
    // );

    // --- #2 METHOD ---
    // match self.entropy_zscore_cutoff {
    //   Some(cutoff) => {
    //     let y_series = series.y();
    //     let signal = match self.entropy_bits {
    //       EntropyBits::One => one_step_entropy_signal(series, self.period)?,
    //       EntropyBits::Two => two_step_entropy_signal(series, self.period)?,
    //       EntropyBits::Three => three_step_entropy_signal(series, self.period)?,
    //     };
    //     let z = zscore(y_series.as_slice(), self.period)?;
    //     if z.abs() > cutoff {
    //       match signal {
    //         EntropySignal::Up => {
    //           enter_long = true;
    //           exit_short = true;
    //         }
    //         EntropySignal::Down => {
    //           enter_short = true;
    //           exit_long = true;
    //         }
    //         _ => {}
    //       }
    //     }
    //   }
    //   None => {
    //     let signal = match self.entropy_bits {
    //       EntropyBits::One => one_step_entropy_signal(series, self.period)?,
    //       EntropyBits::Two => two_step_entropy_signal(series, self.period)?,
    //       EntropyBits::Three => three_step_entropy_signal(series, self.period)?,
    //     };
    //     match signal {
    //       EntropySignal::Up => {
    //         enter_long = true;
    //         exit_short = true;
    //       }
    //       EntropySignal::Down => {
    //         enter_short = true;
    //         exit_long = true;
    //       }
    //       _ => {}
    //     }
    //   }
    // }

    // --- #3 METHOD ---
    // let closes = series.y();
    // let e = shannon_entropy(closes.as_slice(), self.period, self.entropy_bits.patterns());
    // self.e_cache.push(e);
    // let pz = zscore(closes.as_slice(), self.period)?;
    // self.pz_cache.push(pz);
    //
    // let e_series = self.e_cache.vec();
    // // not enough values to compute entropy z-score or price z-score
    // if e_series.len() < self.period || self.pz_cache.vec().len() < self.period {
    //   return Ok(Signals {
    //     enter_long,
    //     exit_long,
    //     enter_short,
    //     exit_short,
    //   });
    // }
    // let ez = zscore(e_series.as_slice(), self.period)?;
    // self.ez_cache.push(ez);
    // // not enough entropy z-score values to compare to price z-score
    // if self.ez_cache.vec().len() < self.period {
    //   return Ok(Signals {
    //     enter_long,
    //     exit_long,
    //     enter_short,
    //     exit_short,
    //   });
    // }
    //
    // if ez.abs() > self.entropy_zscore_cutoff.unwrap_or(0.0) {
    //   let signal = match self.entropy_bits {
    //     EntropyBits::One => one_step_entropy_signal(series, self.period)?,
    //     EntropyBits::Two => two_step_entropy_signal(series, self.period)?,
    //     EntropyBits::Three => three_step_entropy_signal(series, self.period)?,
    //   };
    //   match signal {
    //     EntropySignal::Up => {
    //       enter_long = true;
    //       exit_short = true;
    //     }
    //     EntropySignal::Down => {
    //       enter_short = true;
    //       exit_long = true;
    //     }
    //     _ => {}
    //   }
    // }

    // --- #4 METHOD ---
    let latest_price = *series.y().last().unwrap();
    let mut new_last_signal = self.last_signal.clone();
    // exit position created by last_signal if needed
    if let Some(LastSignal {
      bars_since,
      signal,
      price,
    }) = new_last_signal
    {
      if bars_since > self.entropy_bits.bits() {
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
      let closes = series.y();
      let e = shannon_entropy(closes.as_slice(), self.period, self.entropy_bits.patterns());
      self.e_cache.push(e);
      let pz = zscore(closes.as_slice(), self.period)?;
      self.pz_cache.push(pz);

      let e_series = self.e_cache.vec();
      // not enough values to compute entropy z-score or price z-score
      if e_series.len() < self.period || self.pz_cache.vec().len() < self.period {
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

      if ez.abs() > self.entropy_zscore_cutoff.unwrap_or(0.0) {
        let signal = match self.entropy_bits {
          EntropyBits::One => one_step_entropy_signal(series, self.period)?,
          EntropyBits::Two => two_step_entropy_signal(series, self.period)?,
          EntropyBits::Three => three_step_entropy_signal(series, self.period)?,
        };
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

  pub fn signal(
    &mut self,
    ticker: Option<String>,
    active_trades: &ActiveTrades,
  ) -> anyhow::Result<Vec<Signal>> {
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

        let Data { x: time, y: price } = *self
          .cache
          .front()
          .ok_or(anyhow::anyhow!("No data in cache"))?;

        let mut signals: Vec<Signal> = vec![];

        let id = 0;

        let enter_long_key = Trade::build_key(&ticker, TradeAction::EnterLong, id);
        let active_long = active_trades.get(&enter_long_key);
        let mut has_long = active_long.is_some();

        let enter_short_key = Trade::build_key(&ticker, TradeAction::EnterShort, id);
        let active_short = active_trades.get(&enter_short_key);
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

// ==========================================================================================
//                                 Entropy
// ==========================================================================================

#[test]
fn entropy_one_step() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();

  let clock_start = Time::now();
  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2020, 1, 10, None, None, None);
  let timeframe = "1h";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 100;
  let bits = EntropyBits::One.bits();
  let patterns = EntropyBits::One.patterns();

  let mut longs = 0;
  let mut shorts = 0;
  let mut long_win = 0;
  let mut short_win = 0;
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

    let li = trained.len() - 1;
    b1[li] = trained[li] + 1.0;
    b0[li] = trained[li] - 1.0;

    let length = period + 1;
    let entropy_b1 = shannon_entropy(&b1, length, patterns);
    let entropy_b0 = shannon_entropy(&b0, length, patterns);

    let eli = expected.len() - 1;
    let p0 = series[eli];
    let p1 = series[eli - bits];

    let max = entropy_b1.max(entropy_b0);

    // let up = max == entropy_b1;
    // let down = max == entropy_b0;
    let up = max == entropy_b0;
    let down = max == entropy_b1;

    if up {
      longs += 1;
      if p1 < p0 {
        // long && new price > old price, long wins
        long_win += 1;
        cum_pnl += p0 - p1;
      } else {
        // long && new price < old price, long loses
        cum_pnl += p0 - p1;
      }
    } else if down {
      shorts += 1;
      if p1 > p0 {
        // short && old price > new price, short wins
        short_win += 1;
        cum_pnl += p1 - p0;
      } else {
        // short && old price < new price, short loses
        cum_pnl += p1 - p0;
      }
    }

    pnl_series.push(cum_pnl);
    entropies.push(shannon_entropy(series.as_slice(), length, patterns));
  }
  let avg_entropy = entropies.iter().sum::<f64>() / entropies.len() as f64;
  println!("entropy: {}/{}", trunc!(avg_entropy, 3), patterns);

  let trades = longs + shorts;
  let win_rate = trunc!((long_win + short_win) as f64 / trades as f64 * 100.0, 2);
  let long_win_rate = trunc!(long_win as f64 / longs as f64 * 100.0, 2);
  let short_win_rate = trunc!(short_win as f64 / shorts as f64 * 100.0, 2);
  println!(
    "trades: {}, long: {}/{}, win rate: {}%, long WR: {}%, short WR: ${}, pnl: ${}",
    trades,
    longs,
    longs + shorts,
    win_rate,
    long_win_rate,
    short_win_rate,
    trunc!(cum_pnl, 2)
  );

  println!(
    "finished test in: {}s",
    Time::now().to_unix() - clock_start.to_unix()
  );

  Ok(())
}

/// Uses future bars to confirm whether entropy prediction was correct.
/// This is not to be used directly in a backtest, since future data is impossible.
#[test]
fn entropy_two_step() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();
  let clock_start = Time::now();
  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2025, 1, 1, None, None, None);

  let timeframe = "1h";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 100;
  let bits = EntropyBits::Two.bits();
  let patterns = EntropyBits::Two.patterns();

  let mut longs = 0;
  let mut shorts = 0;
  let mut long_win = 0;
  let mut short_win = 0;
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

    let li = trained.len() - 1;
    b11[li - 1] = trained[li] + 1.0;
    b11[li] = b11[li - 1] + 1.0;
    b00[li - 1] = trained[li] - 1.0;
    b00[li] = b00[li - 1] - 1.0;

    b10[li - 1] = trained[li] + 1.0;
    b10[li] = b10[li - 1] - 1.0;

    b01[li - 1] = trained[li] - 1.0;
    b01[li] = b01[li - 1] + 1.0;

    let length = period + 1;
    let entropy_b11 = shannon_entropy(&b11, length, patterns);
    let entropy_b00 = shannon_entropy(&b00, length, patterns);
    let entropy_b10 = shannon_entropy(&b10, length, patterns);
    let entropy_b01 = shannon_entropy(&b01, length, patterns);

    let eli = expected.len() - 1;
    let p0 = expected[eli];
    let p2 = expected[eli - bits];

    let max = entropy_b11
      .max(entropy_b00)
      .max(entropy_b10)
      .max(entropy_b01);

    // let up = max == entropy_b11;
    // let down = max == entropy_b00;
    let up = max == entropy_b00;
    let down = max == entropy_b11;

    if up {
      longs += 1;
      if p2 < p0 {
        // long && new price > old price, long wins
        long_win += 1;
        cum_pnl += p0 - p2;
      } else {
        // long && new price < old price, long loses
        cum_pnl += p0 - p2;
      }
    } else if down {
      shorts += 1;
      if p2 > p0 {
        // short && old price > new price, short wins
        short_win += 1;
        cum_pnl += p2 - p0;
      } else {
        // short && old price < new price, short loses
        cum_pnl += p2 - p0;
      }
    }

    pnl_series.push(cum_pnl);
    entropies.push(shannon_entropy(trained.as_slice(), length, patterns));
  }
  let avg_entropy = entropies.iter().sum::<f64>() / entropies.len() as f64;
  println!("entropy: {}/{}", trunc!(avg_entropy, 3), patterns);

  let trades = longs + shorts;
  let win_rate = trunc!((long_win + short_win) as f64 / trades as f64 * 100.0, 2);
  let long_win_rate = trunc!(long_win as f64 / longs as f64 * 100.0, 2);
  let short_win_rate = trunc!(short_win as f64 / shorts as f64 * 100.0, 2);
  println!(
    "trades: {}, long: {}/{}, win rate: {}%, long WR: {}%, short WR: ${}, pnl: ${}",
    trades,
    longs,
    longs + shorts,
    win_rate,
    long_win_rate,
    short_win_rate,
    trunc!(cum_pnl, 2)
  );

  println!(
    "finished test in: {}s",
    Time::now().to_unix() - clock_start.to_unix()
  );

  Ok(())
}

#[test]
fn entropy_three_step() -> anyhow::Result<()> {
  use super::*;
  dotenv::dotenv().ok();
  let clock_start = Time::now();
  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2025, 1, 10, None, None, None);
  let timeframe = "1h";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 100;
  let bits = EntropyBits::Three.bits();
  let patterns = EntropyBits::Three.patterns();

  let mut longs = 0;
  let mut shorts = 0;
  let mut long_win = 0;
  let mut short_win = 0;
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

    let li = trained.len() - 1;
    b111[li - 2] = trained[li] + 1.0;
    b111[li - 1] = b111[li - 2] + 1.0;
    b111[li] = b111[li - 1] + 1.0;

    b000[li - 2] = trained[li] - 1.0;
    b000[li - 1] = b000[li - 2] - 1.0;
    b000[li] = b000[li - 1] - 1.0;

    b110[li - 2] = trained[li] + 1.0;
    b110[li - 1] = b110[li - 2] + 1.0;
    b110[li] = b110[li - 1] - 1.0;

    b011[li - 2] = trained[li] - 1.0;
    b011[li - 1] = b011[li - 2] + 1.0;
    b011[li] = b011[li - 1] + 1.0;

    b101[li - 2] = trained[li] + 1.0;
    b101[li - 1] = b101[li - 2] - 1.0;
    b101[li] = b101[li - 1] + 1.0;

    b010[li - 2] = trained[li] - 1.0;
    b010[li - 1] = b010[li - 2] + 1.0;
    b010[li] = b010[li - 1] - 1.0;

    b100[li - 2] = trained[li] + 1.0;
    b100[li - 1] = b100[li - 2] - 1.0;
    b100[li] = b100[li - 1] - 1.0;

    b001[li - 2] = trained[li] - 1.0;
    b001[li - 1] = b001[li - 2] - 1.0;
    b001[li] = b001[li - 1] + 1.0;

    let length = period + 1;
    let entropy_b111 = shannon_entropy(&b111, length, patterns);
    let entropy_b000 = shannon_entropy(&b000, length, patterns);
    let entropy_b110 = shannon_entropy(&b110, length, patterns);
    let entropy_b011 = shannon_entropy(&b011, length, patterns);
    let entropy_b101 = shannon_entropy(&b101, length, patterns);
    let entropy_b010 = shannon_entropy(&b010, length, patterns);
    let entropy_b100 = shannon_entropy(&b100, length, patterns);
    let entropy_b001 = shannon_entropy(&b001, length, patterns);

    let eli = expected.len() - 1;
    let p0 = expected[eli];
    let p3 = expected[eli - bits];

    let max = entropy_b111
      .max(entropy_b000)
      .max(entropy_b110)
      .max(entropy_b011)
      .max(entropy_b101)
      .max(entropy_b010)
      .max(entropy_b100)
      .max(entropy_b001);

    // let up = max == entropy_b111;
    // let down = max == entropy_b000;
    let up = max == entropy_b000;
    let down = max == entropy_b111;

    if up {
      longs += 1;
      if p3 < p0 {
        // long && new price > old price, long wins
        long_win += 1;
        cum_pnl += p0 - p3;
      } else {
        // long && new price < old price, long loses
        cum_pnl += p0 - p3;
      }
    } else if down {
      shorts += 1;
      if p3 > p0 {
        // short && old price > new price, short wins
        short_win += 1;
        cum_pnl += p3 - p0;
      } else {
        // short && old price < new price, short loses
        cum_pnl += p3 - p0;
      }
    }

    pnl_series.push(cum_pnl);
    entropies.push(shannon_entropy(series.as_slice(), period, patterns));
  }
  let avg_entropy = entropies.iter().sum::<f64>() / entropies.len() as f64;
  println!("entropy: {}/{}", trunc!(avg_entropy, 3), patterns);

  let trades = longs + shorts;
  let win_rate = trunc!((long_win + short_win) as f64 / trades as f64 * 100.0, 2);
  let long_win_rate = trunc!(long_win as f64 / longs as f64 * 100.0, 2);
  let short_win_rate = trunc!(short_win as f64 / shorts as f64 * 100.0, 2);
  println!(
    "trades: {}, long: {}/{}, win rate: {}%, long WR: {}%, short WR: ${}, pnl: ${}",
    trades,
    longs,
    longs + shorts,
    win_rate,
    long_win_rate,
    short_win_rate,
    trunc!(cum_pnl, 2)
  );

  println!(
    "finished test in: {}s",
    Time::now().to_unix() - clock_start.to_unix()
  );

  Ok(())
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
  let period = 95;
  let zscore = Some(2.75);

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

  let period_range = 10..500;
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
    let pre_sort = Time::now();
    summaries.sort_by(|a, b| b.pct_roi.partial_cmp(&a.pct_roi).unwrap_or(Ordering::Equal));
    let top_3 = summaries.iter().take(3).collect::<Vec<_>>();
    println!(
      "sort summaries by pct roi in {}ms",
      Time::now().to_unix_ms() - pre_sort.to_unix_ms()
    );

    println!("--- Top by ROI ---");
    for params in top_3 {
      println!(
        "period: {}, zscore: {:?}, roi: {}%, sharpe: {}, dd: {}%",
        params.period, params.zscore, params.pct_roi, params.sharpe_ratio, params.max_drawdown,
      );
    }
  }

  // top 3 by sharpe ratio
  {
    let pre_sort = Time::now();
    summaries.sort_by(|a, b| {
      b.sharpe_ratio
        .partial_cmp(&a.sharpe_ratio)
        .unwrap_or(Ordering::Equal)
    });
    let top_3 = summaries.iter().take(3).collect::<Vec<_>>();
    println!(
      "sort summaries by sharpe ratio in {}ms",
      Time::now().to_unix_ms() - pre_sort.to_unix_ms()
    );

    println!("--- Top by Sharpe ---");
    for params in top_3 {
      println!(
        "period: {}, zscore: {:?}, roi: {}%, sharpe: {}, dd: {}%",
        params.period, params.zscore, params.pct_roi, params.sharpe_ratio, params.max_drawdown,
      );
    }
  }

  // top 3 by drawdown
  {
    let pre_sort = Time::now();
    summaries.sort_by(|a, b| {
      b.max_drawdown
        .partial_cmp(&a.max_drawdown)
        .unwrap_or(Ordering::Equal)
    });
    let top_3 = summaries.iter().take(3).collect::<Vec<_>>();
    println!(
      "sort summaries by max drawdown in {}ms",
      Time::now().to_unix_ms() - pre_sort.to_unix_ms()
    );

    println!("--- Top by Drawdown ---");
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

  let fee = 0.0;
  let slippage = 0.0;
  let stop_loss = None;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = true;

  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2025, 1, 1, None, None, None);
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

// ==========================================================================================
//                                 Shit Test Code
// ==========================================================================================

#[test]
fn shit_test_one_step() -> anyhow::Result<()> {
  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2019, 1, 5, None, None, None);
  let timeframe = "1h";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 15;

  // First method is the backtest strategy
  let mut last_seen = None;
  let mut second_last_seen = None;
  let mut first_method: Vec<(Vec<f64>, EntropySignal)> = vec![];
  let mut ups = 0;
  let mut downs = 0;

  let capacity = period;
  let mut cache = RingBuffer::new(capacity, ticker);

  for (i, data) in btc_series.data().clone().into_iter().enumerate() {
    cache.push(data);
    if cache.vec.len() < capacity {
      continue;
    }
    let series = Dataset::new(cache.vec());
    if last_seen.is_some() {
      if second_last_seen.is_none() {
        println!("#1 second seen at {}: {:?}", i, series.y());
      }
      second_last_seen = last_seen.clone();
    }
    if last_seen.is_none() {
      println!("#1 first seen at {}: {:?}", i, series.y());
    }
    last_seen = Some(series.y());

    let signal = one_step_entropy_signal(series.cloned(), period)?;
    match signal {
      EntropySignal::Up => ups += 1,
      EntropySignal::Down => downs += 1,
      _ => {}
    };

    let y = series.y();
    first_method.push((y, signal));
  }
  if let Some(last_seen) = last_seen {
    println!("#1 last seen: {:?}", last_seen);
  }
  if let Some(second_last_seen) = second_last_seen {
    println!("#1 second_last_seen: {:?}", second_last_seen);
  }
  println!(
    "#1 {}% were up",
    trunc!(ups as f64 / (ups + downs) as f64 * 100.0, 3)
  );

  // Second method is the isolated entropy test
  let second_method: Vec<(Vec<f64>, EntropySignal)> =
    shit_test_one_step_entropy_signals(btc_series, period)?;

  // deep equality check first_method and second_method
  let mut does_match = true;
  if first_method.len() != second_method.len() {
    println!(
      "result lengths do not match, {} != {}",
      first_method.len(),
      second_method.len()
    );
    does_match = false;
  }
  if does_match {
    let checks: Vec<bool> = (0..first_method.len())
      .map(|i| {
        let mut does_match = true;

        let first: &(Vec<f64>, EntropySignal) = &first_method[i];
        let (first_data, first_signal) = first;
        let second: &(Vec<f64>, EntropySignal) = &second_method[i];
        let (second_data, second_signal) = second;

        if first_data.len() != second_data.len() {
          println!(
            "lengths[{}], {} != {}",
            i,
            first_data.len(),
            second_data.len()
          );
          does_match = false;
        }

        if does_match {
          // check if first_signal and second_signal match
          if first_signal != second_signal {
            println!("signals[{}], {:?} != {:?}", i, first_signal, second_signal);
            does_match = false;
          }
        }

        if does_match {
          for (first, second) in first_data.iter().zip(second_data.iter()) {
            if first != second {
              println!("y[{}]", i);
              does_match = false;
              break;
            }
          }
        }
        does_match
      })
      .collect();

    // if not all "checks" are true then set "does_match" to false
    if checks.iter().any(|check| !check) {
      does_match = false;
    }
  }
  match does_match {
    true => {
      println!("results match");
      Ok(())
    }
    false => Err(anyhow::anyhow!("results do not match")),
  }
}

#[test]
fn shit_test_two_step() -> anyhow::Result<()> {
  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2019, 1, 1, None, None, None);
  let timeframe = "1h";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

  let period = 15;

  let capacity = period;
  let mut cache = RingBuffer::new(capacity, ticker);

  // First method is the backtest strategy
  let mut last_seen = None;
  let mut second_last_seen = None;
  let mut first_method: Vec<(Vec<f64>, EntropySignal)> = vec![];
  for (i, data) in btc_series.data().clone().into_iter().enumerate() {
    cache.push(data);
    if cache.vec.len() < capacity {
      continue;
    }
    let series = Dataset::new(cache.vec());
    if last_seen.is_some() {
      if second_last_seen.is_none() {
        println!("#1 second seen at {}: {:?}", i, series.y());
      }
      second_last_seen = last_seen.clone();
    }
    if last_seen.is_none() {
      println!("#1 first seen at {}: {:?}", i, series.y());
    }
    last_seen = Some(series.y());
    let signal = two_step_entropy_signal(series.cloned(), period)?;
    let y = series.y();
    first_method.push((y, signal));
  }
  if let Some(last_seen) = last_seen {
    println!("#1 last seen: {:?}", last_seen);
  }
  if let Some(second_last_seen) = second_last_seen {
    println!("#1 second_last_seen: {:?}", second_last_seen);
  }

  // Second method is the isolated entropy test
  let second_method: Vec<(Vec<f64>, EntropySignal)> =
    shit_test_two_step_entropy_signals(btc_series, period)?;

  // deep equality check first_method and second_method
  let mut does_match = true;
  if first_method.len() != second_method.len() {
    println!(
      "result lengths do not match, {} != {}",
      first_method.len(),
      second_method.len()
    );
    does_match = false;
  }
  if does_match {
    let checks: Vec<bool> = (0..first_method.len())
      .map(|i| {
        let mut does_match = true;

        let first: &(Vec<f64>, EntropySignal) = &first_method[i];
        let (first_data, first_signal) = first;
        let second: &(Vec<f64>, EntropySignal) = &second_method[i];
        let (second_data, second_signal) = second;

        if first_data.len() != second_data.len() {
          println!(
            "lengths[{}], {} != {}",
            i,
            first_data.len(),
            second_data.len()
          );
          does_match = false;
        }

        if does_match {
          // check if first_signal and second_signal match
          if first_signal != second_signal {
            println!("signals[{}], {:?} != {:?}", i, first_signal, second_signal);
            does_match = false;
          }
        }

        if does_match {
          for (first, second) in first_data.iter().zip(second_data.iter()) {
            if first != second {
              println!("y[{}]", i);
              does_match = false;
              break;
            }
          }
        }
        does_match
      })
      .collect();

    // if not all "checks" are true then set "does_match" to false
    if checks.iter().any(|check| !check) {
      does_match = false;
    }
  }
  match does_match {
    true => {
      println!("results match");
      Ok(())
    }
    false => Err(anyhow::anyhow!("results do not match")),
  }
}

#[test]
fn entropy_statistics() -> anyhow::Result<()> {
  let start_time = Time::new(2017, 1, 1, None, None, None);
  let end_time = Time::new(2025, 1, 1, None, None, None);
  let timeframe = "1d";

  let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
  let ticker = "BTC".to_string();
  let btc_series = Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;
  // let btc_series = Dataset::new(btc_series.0.into_iter().take(1000).collect());

  let period = 100;
  let bits = EntropyBits::Two;

  let mut cache = RingBuffer::new(period + bits.bits(), ticker);

  struct Entry {
    pub price: f64,
    pub delta: f64,
    pub price_zscore: f64,
    pub entropy: f64,
    pub entropy_signal: EntropySignal,
  }

  let mut dataset = vec![];
  for data in btc_series.data().clone().into_iter() {
    cache.push(data);
    if cache.vec.len() < period + bits.bits() {
      continue;
    }
    let series = Dataset::new(cache.vec());

    let (in_sample, out_sample) = series.sample(bits.bits());
    assert_eq!(in_sample.len(), period);
    assert_eq!(out_sample.len(), bits.bits());

    let closes = in_sample.y();
    let future = out_sample.0[out_sample.0.len() - 1].y;
    let present = closes[closes.len() - 1];
    let delta = (future - present) / present * 100.0;
    let price_zscore = zscore(closes.as_slice(), period).unwrap();

    let entropy = shannon_entropy(closes.as_slice(), period, bits.patterns());
    let entropy_signal = match bits {
      EntropyBits::One => one_step_entropy_signal(in_sample, period)?,
      EntropyBits::Two => two_step_entropy_signal(in_sample, period)?,
      EntropyBits::Three => three_step_entropy_signal(in_sample, period)?,
    };
    dataset.push(Entry {
      price: present,
      delta,
      price_zscore,
      entropy,
      entropy_signal,
    });
  }

  // iterate and calculate correlation between entropy and delta, and entropy and price zscore
  let p = dataset.iter().map(|entry| entry.price).collect();
  let pz = dataset.iter().map(|entry| entry.price_zscore).collect();
  let d = dataset.iter().map(|entry| entry.delta).collect();
  let dz = rolling_zscore(&d, period).unwrap();
  let e = dataset.iter().map(|entry| entry.entropy).collect();
  let ez = rolling_zscore(&e, period).unwrap();

  let pz_ez_corr = pearson_correlation_coefficient(&pz, &ez).unwrap();
  let roll_pz_ez_corr = rolling_correlation(&pz, &ez, period).unwrap();
  println!("pz : ez,  corr: {}", pz_ez_corr);

  let pz_e_corr = pearson_correlation_coefficient(&pz, &e).unwrap();
  let roll_pz_e_corr = rolling_correlation(&pz, &e, period).unwrap();
  println!("pz : e,  corr: {}", pz_e_corr);

  let p_e_corr = pearson_correlation_coefficient(&p, &e).unwrap();
  let roll_p_e_corr = rolling_correlation(&p, &e, period).unwrap();
  println!("p : e,  corr: {}", p_e_corr);

  let d_ez_corr = pearson_correlation_coefficient(&d, &ez).unwrap();
  let roll_d_ez_corr = rolling_correlation(&dz, &ez, period).unwrap();
  println!("d : ez,  corr: {}", d_ez_corr);

  let pz_dataset = Dataset::from(pz);
  let ez_dataset = Dataset::from(ez);
  let roll_pz_ez_dataset = Dataset::from(roll_pz_ez_corr);
  let roll_pz_e_dataset = Dataset::from(roll_pz_e_corr);
  let roll_p_e_dataset = Dataset::from(roll_p_e_corr);
  let roll_d_ez_dataset = Dataset::from(roll_d_ez_corr);

  Plot::plot(
    vec![
      Series {
        data: roll_pz_ez_dataset.0,
        label: "PZ : EZ Corr".to_string(),
      },
      Series {
        data: roll_pz_e_dataset.0,
        label: "PZ : E Corr".to_string(),
      },
      Series {
        data: roll_p_e_dataset.0,
        label: "P : E Corr".to_string(),
      },
      Series {
        data: roll_d_ez_dataset.0,
        label: "D : EZ Corr".to_string(),
      },
    ],
    "corr.png",
    "Price v Entropy Correlation",
    "Correlation",
    "Time",
    Some(false),
  )?;

  Plot::plot(
    vec![
      Series {
        data: pz_dataset.0,
        label: "Price Z-Score".to_string(),
      },
      Series {
        data: ez_dataset.0,
        label: "Entropy Z-Score".to_string(),
      },
    ],
    "zscore.png",
    "Price Z-Score v Entropy Z-Score",
    "Z Score",
    "Time",
    Some(false),
  )?;

  Ok(())
}
