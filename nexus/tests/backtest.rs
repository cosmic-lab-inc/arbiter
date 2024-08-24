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

    // equity: 10 SOL * $100 = $1000
    // equity after enter long fee: $1000 - 1% = $990
    // enter long with: $990 / $100/SOL = 9.9 SOL
    if time == 0 {
      exit_short = true;
      enter_long = true;
    }
    // ($100 -> $200) exit long with 100% profit
    // equity: 9.9 SOL * $200 = $1980
    // equity after exit long fee: $1980 - 1% = $1960.2
    // equity after enter short fee: $1960.2 - 1% = $1940.598
    // enter short with: $1940.598 / $200/SOL = 9.70299 SOL
    if time == 1 {
      exit_long = true;
      enter_short = true;
    }
    // ($200 -> $250) exit short with 25% loss
    // equity: 9.70299 SOL * $200 = $1940.598
    // loss: 9.70299 SOL * ($250 - $200) = $485.1495
    // equity after loss: $1940.598 - $485.1495 = $1455.4485
    // equity after exit short fee: $1455.4485 - 1% = $1440.894015
    // equity after enter long fee: $1440.894015 - 1% = $1426.485075
    // enter long with: $1426.485075 / $250/SOL = 5.7059403 SOL
    if time == 2 {
      exit_short = true;
      enter_long = true;
    }
    // ($250 -> $400) exit long with 60% profit
    // equity: 5.7059403 SOL * $400 = $2282.37612
    // equity after exit long fee: $2282.37612 - 1% = $2259.552359
    // equity after enter short fee: $2259.552359 - 1% = $2236.956835
    // enter short with: $2236.956835 / $400/SOL = 5.592392088 SOL
    if time == 3 {
      exit_long = true;
      enter_short = true;
    }
    // ($400 -> $300) exit short with 25% profit
    // equity: 5.592392088 SOL * $400 = $2236.956835
    // pnl: 5.592392088 SOL * ($400 - $300) = $559.2392088
    // equity after pnl: $2236.956835 + $559.2392088 = $2796.1960438
    // equity after exit short fee: $2796.1960438 - 1% = $2768.234083362
    // equity after enter long fee: $2768.234083362 - 1% = $2740.551742528
    // enter long with: $2740.551742528 / $300/SOL = 9.135172475 SOL
    if time == 4 {
      exit_short = true;
      enter_long = true;
    }
    // ($300 -> $150) exit long with 50% loss
    // equity: 9.135172475 SOL * $150 = $1370.27587125
    // equity after enter long fee: $1370.27587125 - 1% = $1356.573113
    if time == 5 {
      exit_long = true;
    }

    // ending equity: $1356.573113
    // pnl: $356.573113, or 35.66%

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

  let fee = 1.0;
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
