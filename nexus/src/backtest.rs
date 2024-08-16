#![allow(dead_code)]
#![allow(clippy::unnecessary_cast)]

use crate::{
  trunc, Asset, Assets, Bet, Data, Dataset, Plot, RingBuffer, Series, Signal, Summary, Time, Trade,
  TradeAction, CASH_TICKER,
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
  // let stop_loss = None;
  // let fee = 0.0;
  // let slippage = 0.0;
  // let bet = Bet::Percent(100.0);
  // let leverage = 1;
  // let short_selling = false;

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

  fn enter_trade(&mut self, ticker: &str, price: f64, entry_qty: f64) -> anyhow::Result<()> {
    let qty = entry_qty;

    let available_base_amt = self.assets.get_or_err(CASH_TICKER)?.quantity / price;
    if qty.abs() > available_base_amt {
      // qty = available_base_amt;
      return Err(anyhow::anyhow!(
        "Insufficient funds to enter {} trade, has: {}, needs: {}",
        ticker,
        available_base_amt,
        qty
      ));
    }

    {
      let asset = self.assets.get_mut_or_err(ticker)?;
      asset.quantity += qty.abs();
    }
    {
      let cash = self.assets.get_mut_or_err(CASH_TICKER)?;
      cash.quantity -= qty.abs() * price;
    }
    Ok(())
  }

  fn finalize_trade(
    &mut self,
    date: Time,
    ticker: &str,
    exit_price: f64,
    exit_qty: f64,
  ) -> anyhow::Result<()> {
    let qty = exit_qty;
    let base_amt = self.assets.get_or_err(ticker)?.quantity;

    if qty.abs() > base_amt {
      // qty = *base_amt;
      return Err(anyhow::anyhow!(
        "Insufficient funds to exit {} trade, has: {}, needs: {}",
        ticker,
        base_amt,
        exit_qty
      ));
    }

    let equity_before = self.assets.equity();

    {
      let asset = self.assets.get_mut_or_err(ticker)?;
      asset.quantity -= qty.abs();
    }
    {
      let cash = self.assets.get_mut_or_err(CASH_TICKER)?;
      let quote_fee = qty.abs() * exit_price * (self.fee / 100.0);
      cash.quantity += qty.abs() * exit_price - quote_fee;
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
      let length = first_series.len();

      self.assets.insert(
        CASH_TICKER,
        Asset {
          quantity: self.capital * self.leverage as f64,
          price: 1.0,
        },
      );
      let mut active_trades: HashMap<String, HashSet<Trade>> = HashMap::new();
      for (ticker, series) in series.iter() {
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
      for i in 0..length {
        // Access the i-th element of each vector to simulate getting price update
        // for every ticker at roughly the same time
        let entries = series
          .iter()
          .map(|(ticker, series)| (ticker.clone(), series.clone()))
          .collect::<Vec<(String, Vec<Data>)>>();

        for (ticker, series) in entries.iter() {
          let data = &series[i];
          self.assets.get_mut_or_err(ticker)?.price = data.y;

          // check if stop loss is hit
          if let Some(stop_loss_pct) = self.strategy.stop_loss_pct() {
            let trades = active_trades.entry(ticker.clone()).or_default();
            let mut to_remove = vec![];
            for entry in trades.iter() {
              match entry.side {
                TradeAction::EnterLong => {
                  let pct_diff = (data.y - entry.price) / entry.price * 100.0;
                  if pct_diff < stop_loss_pct * -1.0 {
                    let price_at_stop_loss =
                      entry.price * (1.0 - stop_loss_pct - self.slippage / 100.0);

                    if let Err(e) = self.finalize_trade(
                      Time::from_unix(data.x),
                      ticker,
                      price_at_stop_loss,
                      entry.quantity,
                    ) {
                      log::error!("{}", e);
                      break;
                    }

                    // stop loss exit
                    let exit = Trade {
                      ticker: ticker.clone(),
                      date: Time::from_unix(data.x),
                      side: TradeAction::ExitLong,
                      quantity: entry.quantity,
                      price: price_at_stop_loss,
                    };
                    to_remove.push(entry.clone());
                    self.add_trade(exit, ticker.clone());
                  }
                }
                TradeAction::EnterShort => {
                  // can only be stopped out if entering a short is allowed,
                  // spot markets do not allow short selling
                  if self.short_selling {
                    let pct_diff = (data.y - entry.price) / entry.price * 100.0;
                    if pct_diff > stop_loss_pct {
                      let price_at_stop_loss =
                        entry.price * (1.0 + stop_loss_pct + self.slippage / 100.0);

                      if let Err(e) = self.finalize_trade(
                        Time::from_unix(data.x),
                        ticker,
                        price_at_stop_loss,
                        entry.quantity,
                      ) {
                        log::error!("{}", e);
                        break;
                      }

                      let exit = Trade {
                        ticker: ticker.clone(),
                        date: Time::from_unix(data.x),
                        side: TradeAction::ExitShort,
                        quantity: entry.quantity,
                        price: price_at_stop_loss,
                      };
                      to_remove.push(entry.clone());
                      self.add_trade(exit, ticker.clone());
                    }
                  }
                }
                _ => (),
              }
            }
            for entry in to_remove {
              trades.remove(&entry);
            }
          }

          // place new trades
          let signals =
            self
              .strategy
              .process_data(data.clone(), Some(ticker.clone()), &self.assets)?;
          for signal in signals {
            match signal {
              Signal::EnterLong(entry) => {
                let price = entry.price * (1.0 + self.slippage / 100.0);

                if let Err(e) = self.enter_trade(&entry.ticker, price, entry.quantity) {
                  log::error!("{}", e);
                  break;
                }

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

                if let Err(e) = self.finalize_trade(exit.date, &ticker, exit_price, exit.quantity) {
                  log::error!("{}", e);
                  break;
                }

                let exit = Trade {
                  ticker: ticker.clone(),
                  date: exit.date,
                  side: TradeAction::ExitLong,
                  quantity: exit.quantity,
                  price: exit_price,
                };
                let trades = active_trades.entry(ticker.clone()).or_default();
                trades.remove(&exit);
                self.add_trade(exit, ticker);
              }
              Signal::EnterShort(entry) => {
                if self.short_selling {
                  let price = entry.price * (1.0 - self.slippage / 100.0);
                  if let Err(e) = self.enter_trade(&entry.ticker, price, entry.quantity) {
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
                  if let Err(e) = self.finalize_trade(exit.date, &ticker, exit_price, exit.quantity)
                  {
                    log::error!("{}", e);
                    break;
                  }
                  let exit = Trade {
                    ticker: ticker.clone(),
                    date: exit.date,
                    side: TradeAction::ExitShort,
                    quantity: exit.quantity,
                    price: exit_price,
                  };
                  let trades = active_trades.entry(ticker.clone()).or_default();
                  trades.remove(&exit);
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
    let now = Time::now();
    let summary = self.backtest()?;
    let pct_bah = self.buy_and_hold_pct_roi()?;

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
    println!(
      "{} {} backtest finished in {}s",
      self.strategy.title(),
      timeframe,
      Time::now().to_unix() - now.to_unix()
    );
    Ok(summary)
  }
}
