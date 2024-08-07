use crate::Bar;

#[derive(Debug, Clone, Copy)]
pub enum KagiDirection {
  Up,
  Down,
}

#[derive(Debug, Clone, Copy)]
pub struct Kagi {
  pub direction: KagiDirection,
  pub line: f64,
}
impl Default for Kagi {
  fn default() -> Self {
    Self {
      direction: KagiDirection::Up,
      line: 0.0,
    }
  }
}

impl Kagi {
  pub fn update(kagi: &Kagi, rev_amt: f64, candle: &Bar, _prev_candle: &Bar) -> Self {
    let mut new_kagi = *kagi;

    match kagi.direction {
      KagiDirection::Up => {
        let src = candle.low;
        let diff = candle.close - kagi.line;

        if diff.abs() > rev_amt {
          new_kagi.line = src;
          if diff < 0.0 {
            new_kagi.direction = KagiDirection::Down;
          }
        }
      }
      KagiDirection::Down => {
        let src = candle.high;
        let diff = candle.close - kagi.line;

        if diff.abs() > rev_amt {
          new_kagi.line = src;
          if diff > 0.0 {
            new_kagi.direction = KagiDirection::Up;
          }
        }
      }
    }

    // match kagi.direction {
    //   KagiDirection::Up => {
    //     // candle reverses and drops below kagi line by reversal amount or greater
    //     if _prev_candle.close - candle.close > rev_amt {
    //       // close is beyond reversal amount in opposite kagi direction
    //       new_kagi = Kagi {
    //         line: candle.close,
    //         direction: KagiDirection::Down,
    //       };
    //     }
    //   },
    //   KagiDirection::Down => {
    //     // candle reverses and rises above kagi line by reversal amount or greater
    //     if candle.close - _prev_candle.close > rev_amt {
    //       // close is beyond reversal amount in opposite kagi direction
    //       new_kagi = Kagi {
    //         line: candle.close,
    //         direction: KagiDirection::Up,
    //       };
    //     }
    //   },
    // }

    new_kagi
  }
}
