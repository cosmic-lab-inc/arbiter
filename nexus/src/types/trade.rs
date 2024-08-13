#![allow(clippy::unnecessary_cast)]

use crate::{trunc, Dataset, Time};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Copy, Default)]
pub enum Bet {
  #[default]
  Static,
  Percent(f64),
}

#[derive(Debug, Clone, Copy)]
pub enum Source {
  Open,
  High,
  Low,
  Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeType {
  EnterLong,
  ExitLong,
  EnterShort,
  ExitShort,
}
impl TradeType {
  pub fn is_entry(&self) -> bool {
    matches!(self, TradeType::EnterLong | TradeType::EnterShort)
  }

  pub fn is_exit(&self) -> bool {
    matches!(self, TradeType::ExitLong | TradeType::ExitShort)
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignalInfo {
  pub price: f64,
  pub date: Time,
  pub ticker: String,
  pub quantity: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Signal {
  EnterLong(SignalInfo),
  ExitLong(SignalInfo),
  EnterShort(SignalInfo),
  ExitShort(SignalInfo),
  None,
}

impl Signal {
  pub fn print(&self) -> String {
    match self {
      Signal::EnterLong(data) => {
        format!("🟢🟢 Enter Long {}", data.price)
      }
      Signal::ExitLong(data) => {
        format!("🟢 Exit Long {}", data.price)
      }
      Signal::EnterShort(data) => {
        format!("🔴️🔴️ Enter Short {}", data.price)
      }
      Signal::ExitShort(data) => {
        format!("🔴️ Exit Short {}", data.price)
      }
      Signal::None => "No signal".to_string(),
    }
  }

  #[allow(dead_code)]
  pub fn price(&self) -> Option<f64> {
    match self {
      Signal::EnterLong(info) => Some(info.price),
      Signal::ExitLong(info) => Some(info.price),
      Signal::EnterShort(info) => Some(info.price),
      Signal::ExitShort(info) => Some(info.price),
      Signal::None => None,
    }
  }
}

pub struct Signals {
  pub enter_long: bool,
  pub exit_long: bool,
  pub enter_short: bool,
  pub exit_short: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeAction {
  EnterLong,
  ExitLong,
  EnterShort,
  ExitShort,
}
impl TradeAction {
  pub fn is_entry(&self) -> bool {
    matches!(self, TradeAction::EnterLong | TradeAction::EnterShort)
  }

  pub fn is_exit(&self) -> bool {
    matches!(self, TradeAction::ExitLong | TradeAction::ExitShort)
  }
}
impl Hash for TradeAction {
  fn hash<H: Hasher>(&self, state: &mut H) {
    match self {
      TradeAction::EnterLong => "EnterLong".hash(state),
      TradeAction::ExitLong => "ExitLong".hash(state),
      TradeAction::EnterShort => "EnterShort".hash(state),
      TradeAction::ExitShort => "ExitShort".hash(state),
    }
  }
}

#[derive(Debug, Clone)]
pub struct Trade {
  pub ticker: String,
  pub date: Time,
  pub side: TradeAction,
  /// base asset quantity
  pub quantity: f64,
  pub price: f64,
}
impl PartialEq for Trade {
  fn eq(&self, other: &Self) -> bool {
    self.ticker == other.ticker
      && self.date == other.date
      && self.side == other.side
      && self.quantity == other.quantity
      && self.price == other.price
  }
}
impl Eq for Trade {}
impl Hash for Trade {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.ticker.hash(state);
    self.date.to_unix_ms().hash(state);
    self.side.hash(state);
    self.quantity.to_bits().hash(state);
    self.price.to_bits().hash(state);
  }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PerformanceSummary {
  ticker: String,
  pct_roi: f64,
  quote_roi: f64,
  total_trades: usize,
  win_rate: f64,
  avg_trade_size: f64,
  avg_trade: f64,
  avg_winning_trade: f64,
  avg_losing_trade: f64,
  best_trade: f64,
  worst_trade: f64,
  max_drawdown: f64,
}

#[derive(Debug, Clone)]
pub struct Summary {
  pub cum_quote: HashMap<String, Dataset>,
  pub cum_pct: HashMap<String, Dataset>,
  pub pct_per_trade: HashMap<String, Dataset>,
  pub trades: HashMap<String, Vec<Trade>>,
}
impl Summary {
  pub fn print(&self, ticker: &str) {
    println!("==== {} Backtest Summary ====", ticker);
    println!("Return: {}%", self.pct_roi(ticker));
    println!("Return: ${}", self.quote_roi(ticker));
    println!("Total Trades: {}", self.total_trades(ticker));
    println!("Win Rate: {}%", self.win_rate(ticker));
    println!("Avg Trade Size: ${}", self.avg_trade_size(ticker).unwrap());
    println!("Avg Trade: {}%", self.avg_trade(ticker));
    println!("Avg Winning Trade: {}%", self.avg_winning_trade(ticker));
    println!("Avg Losing Trade: {}%", self.avg_losing_trade(ticker));
    println!("Best Trade: {}%", self.best_trade(ticker));
    println!("Worst Trade: {}%", self.worst_trade(ticker));
    println!("Max Drawdown: {}%", self.max_drawdown(ticker));
  }

  pub fn cum_quote(&self, ticker: &str) -> anyhow::Result<&Dataset> {
    self
      .cum_quote
      .get(ticker)
      .ok_or(anyhow::anyhow!("No cum quote for ticker"))
  }

  pub fn cum_pct(&self, ticker: &str) -> anyhow::Result<&Dataset> {
    self
      .cum_pct
      .get(ticker)
      .ok_or(anyhow::anyhow!("No cum pct for ticker"))
  }

  pub fn pct_per_trade(&self, ticker: &str) -> anyhow::Result<&Dataset> {
    self
      .pct_per_trade
      .get(ticker)
      .ok_or(anyhow::anyhow!("No pct per trade for ticker"))
  }

  pub fn trades(&self, ticker: &str) -> anyhow::Result<&Vec<Trade>> {
    self
      .trades
      .get(ticker)
      .ok_or(anyhow::anyhow!("No trades for ticker"))
  }

  pub fn summarize(&self, ticker: &str) -> anyhow::Result<PerformanceSummary> {
    Ok(PerformanceSummary {
      ticker: ticker.to_string(),
      pct_roi: self.pct_roi(ticker),
      quote_roi: self.quote_roi(ticker),
      total_trades: self.total_trades(ticker),
      win_rate: self.win_rate(ticker),
      avg_trade_size: self.avg_trade_size(ticker)?,
      avg_trade: self.avg_trade(ticker),
      avg_winning_trade: self.avg_winning_trade(ticker),
      avg_losing_trade: self.avg_losing_trade(ticker),
      best_trade: self.best_trade(ticker),
      worst_trade: self.worst_trade(ticker),
      max_drawdown: self.max_drawdown(ticker),
    })
  }

  pub fn total_trades(&self, ticker: &str) -> usize {
    self.cum_pct.get(ticker).unwrap().data().len()
  }

  pub fn avg_trade_size(&self, ticker: &str) -> anyhow::Result<f64> {
    let trades = self
      .trades
      .get(ticker)
      .ok_or(anyhow::anyhow!("No trades for ticker"))?;
    let avg = trades.iter().map(|t| t.price * t.quantity).sum::<f64>() / trades.len() as f64;
    Ok(trunc!(avg, 2))
  }

  pub fn quote_roi(&self, ticker: &str) -> f64 {
    let ending_quote_roi = self.cum_quote.get(ticker).unwrap().data().last().unwrap().y;
    trunc!(ending_quote_roi, 3)
  }

  pub fn pct_roi(&self, ticker: &str) -> f64 {
    let ending_pct_roi = self.cum_pct.get(ticker).unwrap().data().last().unwrap().y;
    trunc!(ending_pct_roi, 3)
  }

  pub fn max_drawdown(&self, ticker: &str) -> f64 {
    let mut max_dd = 0.0;
    let mut peak = self.cum_pct.get(ticker).unwrap().data().first().unwrap().y;

    for point in self.cum_pct.get(ticker).unwrap().data().iter() {
      if point.y > peak {
        peak = point.y;
      } else {
        // 1000 + 14% = 1140
        // 1000 - 35% = 650
        // max drawdown = -35 - 14 = -49
        // 650 - 1140 / 1140 = -0.43
        let y = 1.0 + point.y / 100.0; // 14% = 1.14, -35% = 0.65
        let p = 1.0 + peak / 100.0;
        let dd = (y - p) / p * 100.0;
        // let dd = point.y - peak;
        if dd < max_dd {
          max_dd = dd;
        }
      }
    }
    trunc!(max_dd, 3)
  }

  pub fn avg_trade(&self, ticker: &str) -> f64 {
    let len = self.pct_per_trade.get(ticker).unwrap().data().len();
    let avg_trade = self
      .pct_per_trade
      .get(ticker)
      .unwrap()
      .data()
      .iter()
      .map(|d| d.y)
      .sum::<f64>()
      / len as f64;
    trunc!(avg_trade, 3)
  }

  pub fn avg_winning_trade(&self, ticker: &str) -> f64 {
    let len = self
      .pct_per_trade
      .get(ticker)
      .unwrap()
      .data()
      .iter()
      .filter(|d| d.y > 0.0)
      .count();
    let avg_winning_trade = self
      .pct_per_trade
      .get(ticker)
      .unwrap()
      .data()
      .iter()
      .filter(|d| d.y > 0.0)
      .map(|d| d.y)
      .sum::<f64>()
      / len as f64;
    trunc!(avg_winning_trade, 3)
  }

  pub fn avg_losing_trade(&self, ticker: &str) -> f64 {
    let len = self
      .pct_per_trade
      .get(ticker)
      .unwrap()
      .data()
      .iter()
      .filter(|d| d.y < 0.0)
      .count();
    let avg_losing_trade = self
      .pct_per_trade
      .get(ticker)
      .unwrap()
      .data()
      .iter()
      .filter(|d| d.y < 0.0)
      .map(|d| d.y)
      .sum::<f64>()
      / len as f64;
    trunc!(avg_losing_trade, 3)
  }

  pub fn best_trade(&self, ticker: &str) -> f64 {
    let best_trade = self
      .pct_per_trade
      .get(ticker)
      .unwrap()
      .data()
      .iter()
      .map(|d| d.y)
      .max_by(|a, b| a.partial_cmp(b).unwrap())
      .unwrap();
    trunc!(best_trade, 3)
  }

  pub fn worst_trade(&self, ticker: &str) -> f64 {
    let worst_trade = self
      .pct_per_trade
      .get(ticker)
      .unwrap()
      .data()
      .iter()
      .map(|d| d.y)
      .min_by(|a, b| a.partial_cmp(b).unwrap())
      .unwrap();
    trunc!(worst_trade, 3)
  }

  pub fn win_rate(&self, ticker: &str) -> f64 {
    let len = self.pct_per_trade.get(ticker).unwrap().data().len();
    let win_rate = self
      .pct_per_trade
      .get(ticker)
      .unwrap()
      .data()
      .iter()
      .filter(|d| d.y > 0.0)
      .count() as f64
      / len as f64
      * 100.0;
    trunc!(win_rate, 3)
  }
}

pub const CASH_TICKER: &str = "USD";

/// key is ticker, value is owned asset quantity
#[derive(Debug, Clone)]
pub struct Assets(HashMap<String, f64>);

impl Assets {
  pub fn new() -> Self {
    Self(HashMap::new())
  }

  pub fn cash(&self) -> f64 {
    *self.0.get(CASH_TICKER).unwrap_or(&0.0)
  }

  pub fn equity(&self, prices: HashMap<String, f64>) -> f64 {
    let mut cum_equity = 0.0;
    for (ticker, quantity) in self.0.iter() {
      let price = match ticker == CASH_TICKER {
        true => 1.0,
        false => prices.get(ticker).unwrap_or(&0.0).clone(),
      };
      cum_equity += price * *quantity;
    }
    cum_equity
  }

  pub fn get(&self, ticker: &str) -> Option<&f64> {
    self.0.get(ticker)
  }

  pub fn get_mut(&mut self, ticker: &str) -> Option<&mut f64> {
    self.0.get_mut(ticker)
  }

  pub fn insert(&mut self, ticker: &str, quantity: f64) -> Option<f64> {
    self.0.insert(ticker.to_string(), quantity)
  }

  pub fn remove(&mut self, ticker: &str) -> Option<f64> {
    self.0.remove(ticker)
  }
}
