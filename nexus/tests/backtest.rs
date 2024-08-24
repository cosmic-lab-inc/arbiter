#![allow(dead_code)]

use nexus::*;

#[derive(Debug, Clone)]
pub struct TestBacktest {
  pub ticker: String,
  assets: Positions,
}

impl TestBacktest {
  pub fn new(ticker: String) -> Self {
    Self {
      ticker,
      assets: Positions::default(),
    }
  }

  fn generate_signals(&self, data: Data) -> anyhow::Result<Signals> {
    let mut enter_long = false;
    let mut exit_long = false;
    let mut enter_short = false;
    let mut exit_short = false;

    let Data { x: time, .. } = data;

    // starting equity: $1000
    // simulated time series:
    // [0] = $100
    // [1] = $200
    // [2] = $250
    // [3] = $400
    // [4] = $300
    // [5] = $150

    // equity: $1000
    if time == 0 {
      enter_long = true;
      exit_short = true;
    }
    // ($100 -> $200) exit long with 100% profit
    // equity: $2000
    if time == 1 {
      exit_long = true;
      enter_short = true;
    }
    // ($200 -> $250) exit short with 25% loss
    // equity: $1500
    if time == 2 {
      enter_long = true;
      exit_short = true;
    }
    // ($250 -> $400) exit long with 60% profit
    // equity: $2400
    if time == 3 {
      exit_long = true;
      enter_short = true;
    }
    // ($400 -> $300) exit short with 25% profit
    // equity: $3000
    if time == 4 {
      enter_long = true;
      exit_short = true;
    }
    // ($300 -> $150) exit long with 50% loss
    // equity: $1500
    if time == 5 {
      exit_long = true;
    }

    // ending equity: $1500
    // pnl: $500 or 50%

    Ok(Signals {
      enter_long,
      exit_long,
      enter_short,
      exit_short,
    })
  }

  pub fn signal(
    &mut self,
    data: Data,
    active_trades: &ActiveTrades,
  ) -> anyhow::Result<Vec<Signal>> {
    let Signals {
      enter_long,
      exit_long,
      enter_short,
      exit_short,
    } = self.generate_signals(data.clone())?;

    let Data { x: time, y: price } = data;

    let mut signals: Vec<Signal> = vec![];

    let id = 0;
    let enter_long_key = Trade::build_key(&self.ticker, TradeAction::EnterLong, id);
    let enter_short_key = Trade::build_key(&self.ticker, TradeAction::EnterShort, id);
    let active_long = active_trades.get(&enter_long_key);
    let active_short = active_trades.get(&enter_short_key);

    let mut has_long = active_long.is_some();
    let mut has_short = active_short.is_some();

    let bet = Bet::Percent(100.0);

    if exit_short && has_short {
      if let Some(_entry) = active_short {
        let trade = Signal {
          id,
          price,
          date: Time::from_unix_ms(time),
          ticker: self.ticker.clone(),
          bet: None, // not needed, calculated in backtest using entry
          side: TradeAction::ExitShort,
        };
        signals.push(trade);
        has_short = false;
      }
    }

    if exit_long && has_long {
      if let Some(_entry) = active_long {
        let trade = Signal {
          id,
          price,
          date: Time::from_unix_ms(time),
          ticker: self.ticker.clone(),
          bet: None,
          side: TradeAction::ExitLong,
        };
        signals.push(trade);
        has_long = false;
      }
    }

    if enter_short && !has_short && !has_long {
      let trade = Signal {
        id,
        price,
        date: Time::from_unix_ms(time),
        ticker: self.ticker.clone(),
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
        ticker: self.ticker.clone(),
        bet: Some(bet),
        side: TradeAction::EnterLong,
      };
      signals.push(trade);
    }

    Ok(signals)
  }
}

impl Strategy<Data> for TestBacktest {
  fn process_data(
    &mut self,
    data: Data,
    ticker: Option<String>,
    assets: &Positions,
    active_trades: &ActiveTrades,
  ) -> anyhow::Result<Vec<Signal>> {
    self.assets = assets.clone();
    match ticker {
      Some(ticker) => {
        if self.ticker != ticker {
          Ok(vec![])
        } else {
          self.signal(data, active_trades)
        }
      }
      None => Ok(vec![]),
    }
  }

  fn cache(&self, _: Option<String>) -> Option<&RingBuffer<Data>> {
    None
  }

  fn stop_loss_pct(&self) -> Option<f64> {
    None
  }

  fn title(&self) -> String {
    "test".to_string()
  }
}

#[test]
fn test_backtest() -> anyhow::Result<()> {
  dotenv::dotenv().ok();
  init_logger();

  use nexus::*;

  let fee = 0.0;
  let slippage = 0.0;
  let bet = Bet::Percent(100.0);
  let leverage = 1;
  let short_selling = true;

  let series = Dataset::new(vec![
    Data { x: 0, y: 100.0 },
    Data { x: 1, y: 200.0 },
    Data { x: 2, y: 250.0 },
    Data { x: 3, y: 400.0 },
    Data { x: 4, y: 300.0 },
    Data { x: 5, y: 150.0 },
  ]);
  let ticker = "TEST".to_string();
  let timeframe = "1d";

  let strat = TestBacktest::new(ticker.clone());
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
