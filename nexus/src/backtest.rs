#![allow(dead_code)]
#![allow(clippy::unnecessary_cast)]

use crate::*;
use std::collections::HashMap;
use std::marker::PhantomData;

pub trait Strategy<T>: Clone {
  /// Receives new bar and returns a signal (long, short, or do nothing).
  fn process_data(
    &mut self,
    data: Data,
    ticker: Option<String>,
    assets: &Assets,
    active_trades: &ActiveTrades,
  ) -> anyhow::Result<Vec<Trade>>;
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
    _: Option<String>,
    _: &Assets,
    _: &ActiveTrades,
  ) -> anyhow::Result<Vec<Trade>> {
    Ok(vec![])
  }
  /// Returns a reference to the bar cache
  fn cache(&self, _: Option<String>) -> Option<&RingBuffer<Data>> {
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
  pub signals: HashMap<String, Vec<Trade>>,

  assets: Assets,
  active_trades: ActiveTrades,
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
      bet: Bet::Percent(100.0),
      leverage: 1,
      short_selling: false,
      series: HashMap::new(),
      trades: HashMap::new(),
      signals: HashMap::new(),
      assets: Assets::default(),
      active_trades: ActiveTrades::new(),
      quote: HashMap::new(),
      cum_pct: HashMap::new(),
      cum_quote: HashMap::new(),
      pct_per_trade: HashMap::new(),
      _data: PhantomData,
    }
  }
}

impl<T, S: Strategy<T>> Backtest<T, S> {
  pub fn builder(strategy: S) -> Self {
    Self {
      strategy,
      capital: 1000.0,
      fee: 0.0,
      slippage: 0.0,
      bet: Bet::Percent(100.0),
      leverage: 1,
      short_selling: false,
      series: HashMap::new(),
      trades: HashMap::new(),
      signals: HashMap::new(),
      assets: Assets::default(),
      active_trades: ActiveTrades::new(),
      quote: HashMap::new(),
      cum_pct: HashMap::new(),
      cum_quote: HashMap::new(),
      pct_per_trade: HashMap::new(),
      _data: PhantomData,
    }
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

  pub fn add_signal(&mut self, signal: Trade, ticker: String) {
    let mut signals = self.signals.get(&ticker).unwrap_or(&vec![]).clone();
    signals.push(signal);
    self.signals.insert(ticker, signals);
  }

  pub fn reset(&mut self) {
    self.trades.clear();
    self.signals.clear();
    self.assets.clear();
    self.active_trades.clear();
    self.quote.clear();
    self.cum_pct.clear();
    self.cum_quote.clear();
    self.pct_per_trade.clear();
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

  //
  // Backtest
  //

  fn enter_trade(&mut self, trade: &mut Trade) -> anyhow::Result<()> {
    let cash = self.assets.cash()?.quantity;
    let mut quote = self.bet.value() / 100.0 * cash;
    let fee = quote * self.fee / 100.0;
    quote -= fee;

    match trade.side {
      TradeAction::EnterLong => {
        let price = trade.price * (1.0 + self.slippage / 100.0);
        trade.price = price;

        {
          let qty = quote / price;
          trade.quantity = Some(qty);

          let asset = self.assets.get_mut(&trade.ticker)?;
          asset.quantity += qty;
        }
        {
          let qty = quote / price;
          trade.quantity = Some(qty);

          let cash = self.assets.cash_mut()?;
          cash.quantity -= quote + fee;
        }
      }
      TradeAction::EnterShort => {
        if self.short_selling {
          let price = trade.price * (1.0 - self.slippage / 100.0);
          trade.price = price;

          {
            let qty = quote / price;
            trade.quantity = Some(qty);

            let asset = self.assets.get_mut(&trade.ticker)?;
            asset.quantity -= qty;
          }
          {
            let qty = quote / price;
            trade.quantity = Some(qty);

            let cash = self.assets.cash_mut()?;
            cash.quantity += quote;
          }
        }
      }
      _ => {}
    }

    Ok(())
  }

  fn exit_trade(&mut self, entry_price: f64, trade: &mut Trade) -> anyhow::Result<()> {
    let qty = trade
      .quantity
      .ok_or(anyhow::anyhow!("Trade quantity is None"))?;
    let fee = qty * self.fee / 100.0;

    match trade.side {
      TradeAction::ExitLong => {
        let price = trade.price * (1.0 - self.slippage / 100.0);
        trade.price = price;
        {
          let asset = self.assets.get_mut(&trade.ticker)?;
          asset.quantity -= qty;
        }
        {
          let cash = self.assets.cash_mut()?;
          cash.quantity += (qty - fee) * price;
        }
      }
      TradeAction::ExitShort => {
        if self.short_selling {
          let price = trade.price * (1.0 + self.slippage / 100.0);
          trade.price = price;
          {
            // buy back the same amount that was shorted
            let asset = self.assets.get_mut(&trade.ticker)?;
            asset.quantity += qty;
          }
          {
            // use cash to buy back the amount that was shorted + fee
            let cash = self.assets.cash_mut()?;
            cash.quantity -= (qty + fee) * price;
          }
        }
      }
      _ => {}
    }

    let equity_after = self.assets.equity();
    let pct_pnl = (trade.price - entry_price) / entry_price * 100.0;

    self.cum_quote.get_mut(&trade.ticker).unwrap().push(Data {
      x: trade.date.to_unix_ms(),
      y: trunc!(equity_after - self.capital, 2),
    });
    self.cum_pct.get_mut(&trade.ticker).unwrap().push(Data {
      x: trade.date.to_unix_ms(),
      y: trunc!(equity_after / self.capital * 100.0 - 100.0, 2),
    });
    self
      .pct_per_trade
      .get_mut(&trade.ticker)
      .unwrap()
      .push(Data {
        x: trade.date.to_unix_ms(),
        y: trunc!(pct_pnl, 2),
      });

    if equity_after == 0.0 {
      return Err(anyhow::anyhow!("Bankrupt"));
    }

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
      for (ticker, series) in self.series.iter() {
        let initial_price = series.first().unwrap().y;
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
      let mut bankrupt = false;
      let length = first_series.len();
      for i in 0..length {
        // Access the i-th element of each vector to simulate getting price update
        // for every ticker at roughly the same time
        let all_series = series
          .iter()
          .map(|(ticker, series)| (ticker.clone(), series.clone()))
          .collect::<Vec<(String, Vec<Data>)>>();

        if bankrupt {
          break;
        }

        for (ticker, series) in all_series.iter() {
          let data = &series[i];
          self.assets.get_mut(ticker)?.price = data.y;

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
          let signals = self.strategy.process_data(
            data.clone(),
            Some(ticker.clone()),
            &self.assets,
            &self.active_trades,
          )?;
          for signal in signals {
            match signal.side {
              TradeAction::EnterLong => {
                let mut entry = signal;
                let result = self.enter_trade(&mut entry);

                self.active_trades.insert(entry.clone());
                self.add_trade(entry, ticker.clone());

                if let Err(e) = result {
                  log::error!("{:?}", e);
                  bankrupt = true;
                }
              }
              TradeAction::ExitLong => {
                let mut exit = signal;
                let ticker = exit.ticker.clone();
                let entry_key = Trade::empty(ticker.clone(), TradeAction::EnterLong, exit.id).key();
                let entry_price = self.active_trades.get(&entry_key).unwrap().price;
                let result = self.exit_trade(entry_price, &mut exit);

                self.active_trades.remove(&entry_key);
                self.add_trade(exit, ticker);

                if let Err(e) = result {
                  log::error!("{:?}", e);
                  bankrupt = true;
                }
              }
              TradeAction::EnterShort => {
                if self.short_selling {
                  let mut entry = signal;
                  let result = self.enter_trade(&mut entry);

                  self.active_trades.insert(entry.clone());
                  self.add_trade(entry, ticker.clone());

                  if let Err(e) = result {
                    log::error!("{:?}", e);
                    bankrupt = true;
                  }
                }
              }
              TradeAction::ExitShort => {
                if self.short_selling {
                  let mut exit = signal;
                  let ticker = exit.ticker.clone();
                  let entry_key =
                    Trade::empty(ticker.clone(), TradeAction::EnterShort, exit.id).key();
                  let entry_price = self.active_trades.get(&entry_key).unwrap().price;
                  let result = self.exit_trade(entry_price, &mut exit);

                  self.active_trades.remove(&entry_key);
                  self.add_trade(exit, ticker);

                  if let Err(e) = result {
                    log::error!("{:?}", e);
                    bankrupt = true;
                  }
                }
              }
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

    for (ticker, _) in self.series.iter() {
      if let Some(trades) = self.trades.get(ticker) {
        if trades.len() > 1 {
          summary.print(ticker);
          let bah = pct_bah
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
                data: bah,
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
            Some(summary.pct_roi(ticker) > 0.0),
          )?;
        }
      }
    }
    Ok(summary)
  }
}
