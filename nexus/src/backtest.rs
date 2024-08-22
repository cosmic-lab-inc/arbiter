#![allow(dead_code)]
#![allow(clippy::unnecessary_cast)]

use crate::{
  trunc, Asset, Assets, Bet, Data, Dataset, Plot, RingBuffer, Series, Signal, Summary, Time, Timer,
  Trade, TradeAction, CASH_TICKER,
};
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;

pub trait Strategy<T>: Clone {
  /// Receives new bar and returns a signal (long, short, or do nothing).
  fn process_data(
    &mut self,
    data: Data,
    ticker: Option<String>,
    assets: &Assets,
  ) -> anyhow::Result<Vec<Signal>>;
  /// Returns a reference to the bar cache
  fn cache(&self, ticker: Option<String>) -> Option<&RingBuffer<Data>>;
  fn stop_loss_pct(&self) -> Option<f64>;
  fn title(&self) -> String;
}

#[derive(Debug, Clone, Default)]
pub struct EmptyStrategy;
impl Strategy<f64> for EmptyStrategy {
  /// Receives new bar and returns a signal (long, short, or do nothing).
  fn process_data(
    &mut self,
    _: Data,
    _ticker: Option<String>,
    _assets: &Assets,
  ) -> anyhow::Result<Vec<Signal>> {
    Ok(vec![])
  }
  /// Returns a reference to the bar cache
  fn cache(&self, _ticker: Option<String>) -> Option<&RingBuffer<Data>> {
    None
  }
  fn stop_loss_pct(&self) -> Option<f64> {
    None
  }
  fn title(&self) -> String {
    "empty".to_string()
  }
}
impl EmptyStrategy {
  pub fn new() -> Self {
    Self
  }
}

#[derive(Debug, Clone)]
pub struct Backtest<T, S: Strategy<T>> {
  pub strategy: S,
  pub capital: f64,
  /// Fee in percentage
  pub fee: f64,
  /// Slippage in percentage
  pub slippage: f64,
  /// If compounded, assumes trading profits are 100% reinvested.
  /// If not compounded, assumed trading with initial capital (e.g. $1000 every trade) and not reinvesting profits.
  pub bet: Bet,
  pub leverage: u8,
  /// False if spot trading, true if margin trading which allows short selling
  pub short_selling: bool,
  pub series: HashMap<String, Vec<Data>>,
  pub trades: HashMap<String, Vec<Trade>>,
  pub signals: HashMap<String, Vec<Signal>>,

  assets: Assets,
  quote: HashMap<String, f64>,
  cum_pct: HashMap<String, Vec<Data>>,
  cum_quote: HashMap<String, Vec<Data>>,
  pct_per_trade: HashMap<String, Vec<Data>>,

  _data: PhantomData<T>,
}

impl Default for Backtest<f64, EmptyStrategy> {
  fn default() -> Self {
    Self {
      strategy: EmptyStrategy::new(),
      capital: 1000.0,
      fee: 0.0,
      slippage: 0.0,
      bet: Bet::Static,
      leverage: 1,
      short_selling: false,
      series: HashMap::new(),
      trades: HashMap::new(),
      signals: HashMap::new(),
      assets: Assets::default(),
      quote: HashMap::new(),
      cum_pct: HashMap::new(),
      cum_quote: HashMap::new(),
      pct_per_trade: HashMap::new(),
      _data: PhantomData,
    }
  }
}

impl<T, S: Strategy<T>> Backtest<T, S> {
  pub fn new(
    strategy: S,
    capital: f64,
    fee: f64,
    slippage: f64,
    bet: Bet,
    leverage: u8,
    short_selling: bool,
  ) -> Self {
    Self {
      strategy,
      capital,
      fee,
      slippage,
      bet,
      leverage,
      short_selling,
      series: HashMap::new(),
      trades: HashMap::new(),
      signals: HashMap::new(),
      assets: Assets::default(),
      quote: HashMap::new(),
      cum_pct: HashMap::new(),
      cum_quote: HashMap::new(),
      pct_per_trade: HashMap::new(),
      _data: PhantomData,
    }
  }

  pub fn builder(strategy: S) -> Self {
    Backtest::new(strategy, 1000.0, 0.0, 0.0, Bet::Percent(100.0), 1, false)
  }

  pub fn strategy(mut self, value: S) -> Self {
    self.strategy = value;
    self
  }
  pub fn capital(mut self, value: f64) -> Self {
    self.capital = value;
    self
  }
  pub fn fee(mut self, value: f64) -> Self {
    self.fee = value;
    self
  }
  pub fn slippage(mut self, value: f64) -> Self {
    self.slippage = value;
    self
  }
  pub fn bet(mut self, value: Bet) -> Self {
    self.bet = value;
    self
  }
  pub fn leverage(mut self, value: u8) -> Self {
    self.leverage = value;
    self
  }
  pub fn short_selling(mut self, value: bool) -> Self {
    self.short_selling = value;
    self
  }

  pub fn get_series(&self, ticker: &str) -> anyhow::Result<&Vec<Data>> {
    self
      .series
      .get(ticker)
      .ok_or(anyhow::anyhow!("Ticker {} not found in series", ticker))
  }

  pub fn get_trades(&self, ticker: &str) -> anyhow::Result<&Vec<Trade>> {
    self
      .trades
      .get(ticker)
      .ok_or(anyhow::anyhow!("Ticker {} not found in trades", ticker))
  }

  pub fn add_data(&mut self, data: Data, ticker: String) {
    let mut series = self.series.get(&ticker).unwrap_or(&vec![]).clone();
    series.push(data);
    self.series.insert(ticker, series);
  }

  pub fn add_trade(&mut self, trade: Trade, ticker: String) {
    let mut trades = self.trades.get(&ticker).unwrap_or(&vec![]).clone();
    trades.push(trade);
    self.trades.insert(ticker, trades);
  }

  pub fn add_signal(&mut self, signal: Signal, ticker: String) {
    let mut signals = self.signals.get(&ticker).unwrap_or(&vec![]).clone();
    signals.push(signal);
    self.signals.insert(ticker, signals);
  }

  pub fn reset(&mut self) {
    self.trades.clear();
    self.signals.clear();
  }

  pub fn buy_and_hold_dollar_roi(&mut self) -> anyhow::Result<HashMap<String, Vec<Data>>> {
    let mut all_data = HashMap::new();
    let series = self.series.clone();

    for (ticker, series) in series {
      let first = series.first().unwrap();
      let mut data = vec![];

      for series in series.windows(2) {
        let entry = &series[0];
        let exit = &series[1];

        let pct_pnl = ((exit.y - first.y) / first.y) * self.capital;

        data.push(Data {
          x: entry.x,
          y: pct_pnl,
        });
      }
      all_data.insert(ticker, data);
    }
    Ok(all_data)
  }

  pub fn buy_and_hold_pct_roi(&mut self) -> anyhow::Result<HashMap<String, Vec<Data>>> {
    let mut all_data = HashMap::new();
    let series = self.series.clone();

    for (ticker, series) in series {
      let first = series.first().unwrap();
      let mut data = vec![];

      for series in series.windows(2) {
        let entry = &series[0];
        let exit = &series[1];
        let pct_pnl = ((exit.y - first.y) / first.y) * 100.0;

        data.push(Data {
          x: entry.x,
          y: pct_pnl,
        });
      }
      all_data.insert(ticker, data);
    }
    Ok(all_data)
  }

  pub fn binary_backtest(&mut self) -> anyhow::Result<Summary> {
    let static_capital = self.capital * self.leverage as f64;
    let initial_capital = self.capital;

    let mut cum_capital: HashMap<String, f64> = HashMap::new();

    let all_series = self.series.clone();
    if let Some((_, first_series)) = all_series.iter().next() {
      self.assets.insert(
        CASH_TICKER,
        Asset {
          quantity: self.capital * self.leverage as f64,
          price: 1.0,
        },
      );
      let mut active_trades: HashMap<String, Option<Trade>> = HashMap::new();
      for (ticker, series) in self.series.iter() {
        let initial_price = series.first().unwrap().y;
        // populate active trades with None values for each ticker so getter doesn't panic
        active_trades.insert(ticker.clone(), None);
        // populate with empty vec for each ticker so getter doesn't panic
        self.trades.insert(ticker.clone(), vec![]);

        self.assets.insert(
          ticker,
          Asset {
            quantity: 0.0,
            price: initial_price,
          },
        );

        // populate all tickers with starting values
        cum_capital.insert(ticker.clone(), self.capital * self.leverage as f64);
        self.quote.insert(ticker.clone(), 0.0);
        self.cum_pct.insert(ticker.clone(), vec![]);
        self.cum_quote.insert(ticker.clone(), vec![]);
        self.pct_per_trade.insert(ticker.clone(), vec![]);
      }

      // Iterate over the index of each series
      let length = first_series.len();
      for i in 0..length {
        // Access the i-th element of each vector to simulate getting price update
        // for every ticker at roughly the same time
        for (ticker, series) in self.series.clone().iter() {
          let data = &series[i];

          // check if stop loss is hit
          if let (Some(entry), Some(stop_loss_pct)) = (
            active_trades.get(ticker).unwrap(),
            self.strategy.stop_loss_pct(),
          ) {
            match entry.side {
              TradeAction::EnterLong => {
                let price = entry.price * (1.0 + self.slippage / 100.0);

                let pct_diff = (data.y - price) / price * 100.0;
                if pct_diff < stop_loss_pct * -1.0 {
                  let price_at_stop_loss = price * (1.0 - stop_loss_pct / 100.0);
                  // longs are stopped out by the low
                  let pct_pnl = (price_at_stop_loss - price) / price * 100.0;
                  let position_size = match self.bet {
                    Bet::Static => static_capital,
                    Bet::Percent(pct) => *cum_capital.get(ticker).unwrap() * pct / 100.0,
                  };

                  // add entry trade with updated quantity
                  let quantity = position_size / price;
                  let updated_entry = Trade {
                    ticker: ticker.clone(),
                    date: entry.date,
                    side: entry.side,
                    quantity,
                    price,
                  };
                  self.add_trade(updated_entry, ticker.clone());

                  // fee on trade entry capital
                  let entry_fee = position_size.abs() * (self.fee / 100.0);
                  let cum_capital = cum_capital.get_mut(ticker).unwrap();
                  *cum_capital -= entry_fee;

                  // fee on profit made
                  let mut quote_pnl = pct_pnl / 100.0 * position_size;
                  let profit_fee = quote_pnl.abs() * (self.fee / 100.0);
                  quote_pnl -= profit_fee;

                  *cum_capital += quote_pnl;
                  let quote = self.quote.get_mut(ticker).unwrap();
                  *quote += quote_pnl;

                  self.cum_quote.get_mut(ticker).unwrap().push(Data {
                    x: entry.date.to_unix_ms(),
                    y: trunc!(*quote, 2),
                  });
                  self.cum_pct.get_mut(ticker).unwrap().push(Data {
                    x: entry.date.to_unix_ms(),
                    y: trunc!(*cum_capital / initial_capital * 100.0 - 100.0, 2),
                  });
                  self.pct_per_trade.get_mut(ticker).unwrap().push(Data {
                    x: entry.date.to_unix_ms(),
                    y: trunc!(pct_pnl, 2),
                  });

                  // stop loss exit
                  let quantity = position_size / price_at_stop_loss;
                  let exit = Trade {
                    ticker: ticker.clone(),
                    date: Time::from_unix(data.x),
                    side: TradeAction::ExitLong,
                    quantity,
                    price: price_at_stop_loss,
                  };
                  active_trades.insert(ticker.clone(), None);
                  self.add_trade(exit, ticker.clone());
                }
              }
              TradeAction::EnterShort => {
                // can only be stopped out if entering a short is allowed,
                // spot markets do not allow short selling
                if self.short_selling {
                  let price = entry.price * (1.0 - self.slippage / 100.0);
                  let pct_diff = (data.y - price) / price * 100.0;
                  if pct_diff > stop_loss_pct {
                    let price_at_stop_loss = price * (1.0 + stop_loss_pct / 100.0);
                    // longs are stopped out by the low
                    let pct_pnl = (price_at_stop_loss - price) / price * -1.0 * 100.0;
                    let position_size = match self.bet {
                      Bet::Static => static_capital,
                      Bet::Percent(pct) => *cum_capital.get(ticker).unwrap() * pct / 100.0,
                    };

                    // add entry trade with updated quantity
                    let quantity = position_size / price;
                    let updated_entry = Trade {
                      ticker: ticker.clone(),
                      date: entry.date,
                      side: entry.side,
                      quantity,
                      price,
                    };
                    self.add_trade(updated_entry, ticker.clone());

                    // fee on trade entry capital
                    let entry_fee = position_size.abs() * (self.fee / 100.0);
                    let cum_capital = cum_capital.get_mut(ticker).unwrap();
                    *cum_capital -= entry_fee;
                    // fee on profit made
                    let mut quote_pnl = pct_pnl / 100.0 * position_size;
                    let profit_fee = quote_pnl.abs() * (self.fee / 100.0);
                    quote_pnl -= profit_fee;

                    *cum_capital += quote_pnl;
                    let quote = self.quote.get_mut(ticker).unwrap();
                    *quote += quote_pnl;

                    self.cum_quote.get_mut(ticker).unwrap().push(Data {
                      x: entry.date.to_unix_ms(),
                      y: trunc!(*quote, 2),
                    });
                    self.cum_pct.get_mut(ticker).unwrap().push(Data {
                      x: entry.date.to_unix_ms(),
                      y: trunc!(*cum_capital / initial_capital * 100.0 - 100.0, 2),
                    });
                    self.pct_per_trade.get_mut(ticker).unwrap().push(Data {
                      x: entry.date.to_unix_ms(),
                      y: trunc!(pct_pnl, 2),
                    });

                    // stop loss exit
                    let quantity = position_size / price_at_stop_loss;
                    let exit = Trade {
                      ticker: ticker.clone(),
                      date: Time::from_unix(data.x),
                      side: TradeAction::ExitShort,
                      quantity,
                      price: price_at_stop_loss,
                    };
                    active_trades.insert(ticker.clone(), None);
                    self.add_trade(exit, ticker.clone());
                  }
                }
              }
              _ => (),
            }
          }

          // place new trade if signal is present
          // let equity = cum_capital.get(ticker).unwrap();
          let signals =
            self
              .strategy
              .process_data(data.clone(), Some(ticker.clone()), &self.assets)?;
          for signal in signals {
            match signal {
              Signal::EnterLong(info) => {
                let price = info.price * (1.0 + self.slippage / 100.0);
                // only place if no active trade to prevent pyramiding
                if active_trades.get(&info.ticker).unwrap().is_none() {
                  let trade = Trade {
                    ticker: info.ticker.clone(),
                    date: info.date,
                    side: TradeAction::EnterLong,
                    quantity: 0.0, // quantity doesn't matter, since exit trade computes it
                    price,
                  };
                  active_trades.insert(info.ticker.clone(), Some(trade.clone()));
                }
              }
              Signal::ExitLong(info) => {
                if let Some(entry) = active_trades.get(&info.ticker).unwrap() {
                  if entry.side == TradeAction::EnterLong {
                    let exit_price = info.price * (1.0 - self.slippage / 100.0);
                    let pct_pnl = (exit_price - entry.price) / entry.price * 100.0;
                    let position_size = match self.bet {
                      Bet::Static => static_capital,
                      Bet::Percent(pct) => *cum_capital.get(&info.ticker).unwrap() * pct / 100.0,
                    };

                    let quantity = position_size / entry.price;
                    let updated_entry = Trade {
                      ticker: ticker.clone(),
                      date: entry.date,
                      side: entry.side,
                      quantity,
                      price: entry.price,
                    };
                    self.add_trade(updated_entry, info.ticker.clone());

                    // fee on trade entry capital
                    let entry_fee = position_size.abs() * (self.fee / 100.0);
                    let cum_capital = cum_capital.get_mut(&info.ticker).unwrap();
                    *cum_capital -= entry_fee;
                    // fee on profit made
                    let mut quote_pnl = pct_pnl / 100.0 * position_size;
                    let profit_fee = quote_pnl.abs() * (self.fee / 100.0);
                    quote_pnl -= profit_fee;

                    *cum_capital += quote_pnl;
                    let quote = self.quote.get_mut(&info.ticker).unwrap();
                    *quote += quote_pnl;

                    self.cum_quote.get_mut(&info.ticker).unwrap().push(Data {
                      x: entry.date.to_unix_ms(),
                      y: trunc!(*quote, 2),
                    });
                    self.cum_pct.get_mut(&info.ticker).unwrap().push(Data {
                      x: entry.date.to_unix_ms(),
                      y: trunc!(*cum_capital / initial_capital * 100.0 - 100.0, 2),
                    });
                    self
                      .pct_per_trade
                      .get_mut(&info.ticker)
                      .unwrap()
                      .push(Data {
                        x: entry.date.to_unix_ms(),
                        y: trunc!(pct_pnl, 2),
                      });

                    let quantity = position_size / exit_price;
                    let exit = Trade {
                      ticker: info.ticker.clone(),
                      date: info.date,
                      side: TradeAction::ExitLong,
                      quantity,
                      price: exit_price,
                    };
                    active_trades.insert(info.ticker.clone(), None);
                    self.add_trade(exit, info.ticker.clone());
                  }
                }
              }
              Signal::EnterShort(info) => {
                // only place if no active trade to prevent pyramiding
                // todo: allow pyramiding to enable hedging
                if active_trades.get(&info.ticker).unwrap().is_none() && self.short_selling {
                  let price = info.price * (1.0 - self.slippage / 100.0);
                  let trade = Trade {
                    ticker: info.ticker.clone(),
                    date: info.date,
                    side: TradeAction::EnterShort,
                    quantity: 0.0, // quantity doesn't matter, since exit trade recomputes it
                    price,
                  };
                  active_trades.insert(info.ticker.clone(), Some(trade.clone()));
                }
              }
              Signal::ExitShort(info) => {
                if let Some(entry) = active_trades.get(&info.ticker).unwrap() {
                  if entry.side == TradeAction::EnterShort && self.short_selling {
                    let exit_price = info.price * (1.0 + self.slippage / 100.0);
                    let pct_pnl = (exit_price - entry.price) / entry.price * -1.0 * 100.0;
                    let position_size = match self.bet {
                      Bet::Static => static_capital,
                      Bet::Percent(pct) => *cum_capital.get(&info.ticker).unwrap() * pct / 100.0,
                    };

                    let quantity = position_size / entry.price;
                    let updated_entry = Trade {
                      ticker: info.ticker.clone(),
                      date: entry.date,
                      side: entry.side,
                      quantity,
                      price: entry.price,
                    };
                    self.add_trade(updated_entry, info.ticker.clone());

                    // fee on trade entry capital
                    let entry_fee = position_size.abs() * (self.fee / 100.0);
                    let cum_capital = cum_capital.get_mut(&info.ticker).unwrap();
                    *cum_capital -= entry_fee;
                    // fee on profit made
                    let mut quote_pnl = pct_pnl / 100.0 * position_size;
                    let profit_fee = quote_pnl.abs() * (self.fee / 100.0);
                    quote_pnl -= profit_fee;

                    *cum_capital += quote_pnl;
                    let quote = self.quote.get_mut(&info.ticker).unwrap();
                    *quote += quote_pnl;

                    self.cum_quote.get_mut(&info.ticker).unwrap().push(Data {
                      x: entry.date.to_unix_ms(),
                      y: trunc!(*quote, 2),
                    });
                    self.cum_pct.get_mut(&info.ticker).unwrap().push(Data {
                      x: entry.date.to_unix_ms(),
                      y: trunc!(*cum_capital / initial_capital * 100.0 - 100.0, 2),
                    });
                    self
                      .pct_per_trade
                      .get_mut(&info.ticker)
                      .unwrap()
                      .push(Data {
                        x: entry.date.to_unix_ms(),
                        y: trunc!(pct_pnl, 2),
                      });

                    let quantity = position_size / exit_price;
                    let exit = Trade {
                      ticker: info.ticker.clone(),
                      date: info.date,
                      side: TradeAction::ExitShort,
                      quantity,
                      price: exit_price,
                    };
                    active_trades.insert(info.ticker.clone(), None);
                    self.add_trade(exit, info.ticker.clone());
                  }
                }
              }
              _ => (),
            }
          }
        }
      }
    }

    let cum_quote = self
      .cum_quote
      .iter()
      .map(|(ticker, data)| (ticker.clone(), Dataset::new(data.clone())))
      .collect();
    let cum_pct = self
      .cum_pct
      .iter()
      .map(|(ticker, data)| (ticker.clone(), Dataset::new(data.clone())))
      .collect();
    let pct_per_trade = self
      .pct_per_trade
      .iter()
      .map(|(ticker, data)| (ticker.clone(), Dataset::new(data.clone())))
      .collect();
    Ok(Summary {
      cum_quote,
      cum_pct,
      pct_per_trade,
      trades: self.trades.clone(),
    })
  }

  //
  // Backtest with Rebalance
  //

  fn enter_trade(
    &mut self,
    ticker: &str,
    price: f64,
    entry_qty: f64,
    action: TradeAction,
  ) -> anyhow::Result<()> {
    let mut qty = entry_qty.abs();

    match action {
      TradeAction::EnterLong => {
        let available_base_amt = self.assets.get_or_err(CASH_TICKER)?.quantity / price;
        if qty.abs() > available_base_amt {
          qty = available_base_amt;
          // return Err(anyhow::anyhow!(
          //   "Insufficient funds to enter {} trade, has: {}, needs: {}",
          //   ticker,
          //   available_base_amt,
          //   qty
          // ));
        }
        {
          let asset = self.assets.get_mut_or_err(ticker)?;
          let fee = qty.abs() * (self.fee / 100.0);
          asset.quantity += qty.abs() - fee;
        }
        {
          let cash = self.assets.get_mut_or_err(CASH_TICKER)?;
          cash.quantity -= qty * price;
        }
      }
      TradeAction::EnterShort => {
        if self.short_selling {
          {
            let asset = self.assets.get_mut_or_err(ticker)?;
            asset.quantity -= qty;
          }
          {
            let cash = self.assets.get_mut_or_err(CASH_TICKER)?;
            let fee = qty.abs() * (self.fee / 100.0);
            cash.quantity += (qty.abs() - fee) * price;
          }
        }
      }
      _ => {}
    }

    Ok(())
  }

  // todo: assign entry an id as Option<u8> and use that with the exit to determine pnl and delete entries after closed
  fn exit_trade(
    &mut self,
    date: Time,
    ticker: &str,
    exit_price: f64,
    exit_qty: f64,
    action: TradeAction,
  ) -> anyhow::Result<()> {
    let mut qty = exit_qty;
    let base_amt = self.assets.get_or_err(ticker)?.quantity;
    let equity_before = self.assets.equity();

    match action {
      TradeAction::ExitLong => {
        // only matters if unable to short sell (negative base amount)
        if base_amt > 0.0 && qty.abs() > base_amt {
          qty = base_amt;
          // return Err(anyhow::anyhow!(
          //   "Insufficient funds to exit {} trade, has: {}, needs: {}",
          //   ticker,
          //   base_amt,
          //   exit_qty
          // ));
        }
        {
          let asset = self.assets.get_mut_or_err(ticker)?;
          asset.quantity -= qty;
        }
        {
          let cash = self.assets.get_mut_or_err(CASH_TICKER)?;
          let fee = qty.abs() * (self.fee / 100.0);
          cash.quantity += (qty.abs() - fee) * exit_price;
        }
      }
      TradeAction::ExitShort => {
        {
          let asset = self.assets.get_mut_or_err(ticker)?;
          let fee = qty.abs() * (self.fee / 100.0);
          asset.quantity += qty.abs() - fee;
        }
        {
          let cash = self.assets.get_mut_or_err(CASH_TICKER)?;
          cash.quantity -= qty * exit_price;
        }
      }
      _ => {}
    }

    let equity_after = self.assets.equity();
    let pct_pnl = (equity_after - equity_before) / equity_before * 100.0;

    self.cum_quote.get_mut(ticker).unwrap().push(Data {
      x: date.to_unix_ms(),
      y: trunc!(equity_after - self.capital, 2),
    });
    self.cum_pct.get_mut(ticker).unwrap().push(Data {
      x: date.to_unix_ms(),
      y: trunc!(equity_after / self.capital * 100.0 - 100.0, 2),
    });
    self.pct_per_trade.get_mut(ticker).unwrap().push(Data {
      x: date.to_unix_ms(),
      y: trunc!(pct_pnl, 2),
    });
    Ok(())
  }

  pub fn backtest(&mut self) -> anyhow::Result<Summary> {
    let series = self.series.clone();

    if let Some((_, first_series)) = series.iter().next() {
      self.assets.insert(
        CASH_TICKER,
        Asset {
          quantity: self.capital * self.leverage as f64,
          price: 1.0,
        },
      );
      let mut active_trades: HashMap<String, HashSet<Trade>> = HashMap::new();
      for (ticker, series) in self.series.iter() {
        let initial_price = series.first().unwrap().y;
        // populate active trades with None values for each ticker so getter doesn't panic
        active_trades.insert(ticker.clone(), HashSet::new());
        // populate with empty vec for each ticker so getter doesn't panic
        self.trades.insert(ticker.clone(), vec![]);
        // populate all tickers with starting values
        self.assets.insert(
          ticker,
          Asset {
            quantity: 0.0,
            price: initial_price,
          },
        );
        self.quote.insert(ticker.clone(), 0.0);
        self.cum_pct.insert(ticker.clone(), vec![]);
        self.cum_quote.insert(ticker.clone(), vec![]);
        self.pct_per_trade.insert(ticker.clone(), vec![]);
      }

      // Iterate over the index of each series
      let length = first_series.len();
      for i in 0..length {
        // Access the i-th element of each vector to simulate getting price update
        // for every ticker at roughly the same time
        let all_series = series
          .iter()
          .map(|(ticker, series)| (ticker.clone(), series.clone()))
          .collect::<Vec<(String, Vec<Data>)>>();

        for (ticker, series) in all_series.iter() {
          let data = &series[i];
          self.assets.get_mut_or_err(ticker)?.price = data.y;

          // todo: figure out how to associate entry with exit when there are multiple entries
          // check if stop loss is hit
          // if let Some(stop_loss_pct) = self.strategy.stop_loss_pct() {
          //   let trades = active_trades.entry(ticker.clone()).or_default();
          //   let mut to_remove = vec![];
          //   for entry in trades.iter() {
          //     match entry.side {
          //       TradeAction::EnterLong => {
          //         let pct_diff = (data.y - entry.price) / entry.price * 100.0;
          //         if pct_diff < stop_loss_pct * -1.0 {
          //           let price_at_stop_loss =
          //             entry.price * (1.0 - stop_loss_pct - self.slippage / 100.0);
          //
          //           self.exit_trade(
          //             Time::from_unix(data.x),
          //             ticker,
          //             price_at_stop_loss,
          //             entry.quantity,
          //             TradeAction::ExitLong
          //           )?;
          //
          //           // stop loss exit
          //           let exit = Trade {
          //             ticker: ticker.clone(),
          //             date: Time::from_unix(data.x),
          //             side: TradeAction::ExitLong,
          //             quantity: entry.quantity,
          //             price: price_at_stop_loss,
          //           };
          //           // todo: this isn't working since has of entry
          //           to_remove.push(entry.clone());
          //           self.add_trade(exit, ticker.clone());
          //         }
          //       }
          //       TradeAction::EnterShort => {
          //         // can only be stopped out if entering a short is allowed,
          //         // spot markets do not allow short selling
          //         if self.short_selling {
          //           let pct_diff = (data.y - entry.price) / entry.price * 100.0;
          //           if pct_diff > stop_loss_pct {
          //             let price_at_stop_loss =
          //               entry.price * (1.0 + stop_loss_pct + self.slippage / 100.0);
          //
          //             self.exit_trade(
          //               Time::from_unix(data.x),
          //               ticker,
          //               price_at_stop_loss,
          //               entry.quantity,
          //             )?;
          //
          //             let exit = Trade {
          //               ticker: ticker.clone(),
          //               date: Time::from_unix(data.x),
          //               side: TradeAction::ExitShort,
          //               quantity: entry.quantity,
          //               price: price_at_stop_loss,
          //             };
          //             to_remove.push(entry.clone());
          //             self.add_trade(exit, ticker.clone());
          //           }
          //         }
          //       }
          //       _ => (),
          //     }
          //   }
          //   for entry in to_remove {
          //     trades.remove(&entry);
          //   }
          // }

          // place new trades
          let signals =
            self
              .strategy
              .process_data(data.clone(), Some(ticker.clone()), &self.assets)?;
          for signal in signals {
            match signal {
              Signal::EnterLong(entry) => {
                let price = entry.price * (1.0 + self.slippage / 100.0);

                self.enter_trade(&entry.ticker, price, entry.quantity, TradeAction::EnterLong)?;

                let trade = Trade {
                  ticker: entry.ticker.clone(),
                  date: entry.date,
                  side: TradeAction::EnterLong,
                  quantity: entry.quantity,
                  price,
                };
                let trades = active_trades.entry(ticker.clone()).or_default();
                trades.insert(trade.clone());
                self.add_trade(trade, ticker.clone());
              }
              Signal::ExitLong(exit) => {
                let ticker = exit.ticker.clone();
                let exit_price = exit.price * (1.0 - self.slippage / 100.0);

                self.exit_trade(
                  exit.date,
                  &ticker,
                  exit_price,
                  exit.quantity,
                  TradeAction::ExitLong,
                )?;

                let exit = Trade {
                  ticker: ticker.clone(),
                  date: exit.date,
                  side: TradeAction::ExitLong,
                  quantity: exit.quantity,
                  price: exit_price,
                };
                // let trades = active_trades.entry(ticker.clone()).or_default();
                // trades.remove(&exit);
                self.add_trade(exit, ticker);
              }
              Signal::EnterShort(entry) => {
                if self.short_selling {
                  let price = entry.price * (1.0 - self.slippage / 100.0);
                  if let Err(e) = self.enter_trade(
                    &entry.ticker,
                    price,
                    entry.quantity,
                    TradeAction::EnterShort,
                  ) {
                    log::error!("{}", e);
                    break;
                  }
                  let trade = Trade {
                    ticker: entry.ticker.clone(),
                    date: entry.date,
                    side: TradeAction::EnterShort,
                    quantity: entry.quantity,
                    price,
                  };
                  let trades = active_trades.entry(ticker.clone()).or_default();
                  trades.insert(trade.clone());
                }
              }
              Signal::ExitShort(exit) => {
                if self.short_selling {
                  let ticker = exit.ticker.clone();
                  let exit_price = exit.price * (1.0 + self.slippage / 100.0);
                  self.exit_trade(
                    exit.date,
                    &ticker,
                    exit_price,
                    exit.quantity,
                    TradeAction::ExitShort,
                  )?;

                  let exit = Trade {
                    ticker: ticker.clone(),
                    date: exit.date,
                    side: TradeAction::ExitShort,
                    quantity: exit.quantity,
                    price: exit_price,
                  };
                  // let trades = active_trades.entry(ticker.clone()).or_default();
                  // trades.remove(&exit);
                  self.add_trade(exit, ticker);
                }
              }
              _ => (),
            }
          }
        }
      }
    }

    let cum_quote = self
      .cum_quote
      .iter()
      .map(|(ticker, data)| (ticker.clone(), Dataset::new(data.clone())))
      .collect();
    let cum_pct = self
      .cum_pct
      .iter()
      .map(|(ticker, data)| (ticker.clone(), Dataset::new(data.clone())))
      .collect();
    let pct_per_trade = self
      .pct_per_trade
      .iter()
      .map(|(ticker, data)| (ticker.clone(), Dataset::new(data.clone())))
      .collect();
    Ok(Summary {
      cum_quote,
      cum_pct,
      pct_per_trade,
      trades: self.trades.clone(),
    })
  }

  pub fn execute(&mut self, plot_title: &str, timeframe: &str) -> anyhow::Result<Summary> {
    let backtest_timer = Timer::new();
    let summary = self.backtest()?;
    println!(
      "{} {} backtested in {}ms",
      self.strategy.title(),
      timeframe,
      backtest_timer.millis()
    );
    let pct_bah = self.buy_and_hold_pct_roi()?;

    let plot_timer = Timer::new();
    for (ticker, _) in self.series.iter() {
      if let Some(trades) = self.trades.get(ticker) {
        if trades.len() > 1 {
          summary.print(ticker);

          let ticker_bah = pct_bah
            .get(ticker)
            .ok_or(anyhow::anyhow!("Buy and hold not found for ticker"))?
            .clone();
          Plot::plot(
            vec![
              Series {
                data: summary.cum_pct(ticker)?.data().clone(),
                label: "Strategy".to_string(),
              },
              Series {
                data: ticker_bah,
                label: "Buy & Hold".to_string(),
              },
            ],
            &format!(
              "{}_{}_{}_backtest.png",
              self.strategy.title(),
              ticker.to_ascii_lowercase(),
              timeframe
            ),
            &format!("{} {}", ticker, plot_title),
            "% ROI",
            "Unix Millis",
            None,
          )?;
        }
      }
    }
    println!("plotted backtests in {}ms", plot_timer.millis());
    Ok(summary)
  }
}
