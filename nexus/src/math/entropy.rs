use crate::{trunc, Dataset};

#[derive(Debug, Copy, Clone)]
pub enum EntropyBits {
  One,
  Two,
  Three,
  Four,
}
impl EntropyBits {
  pub fn bits(&self) -> usize {
    match self {
      EntropyBits::One => 1,
      EntropyBits::Two => 2,
      EntropyBits::Three => 3,
      EntropyBits::Four => 4,
    }
  }

  pub fn patterns(&self) -> usize {
    match self {
      EntropyBits::One => 2,
      EntropyBits::Two => 3,
      EntropyBits::Three => 4,
      EntropyBits::Four => 5,
    }
  }
}

#[derive(Debug, Clone, Copy)]
pub enum EntropySignal {
  Up,
  Down,
  None,
}
impl PartialEq for EntropySignal {
  fn eq(&self, other: &Self) -> bool {
    matches!(
      (self, other),
      (EntropySignal::Up, EntropySignal::Up)
        | (EntropySignal::Down, EntropySignal::Down)
        | (EntropySignal::None, EntropySignal::None)
    )
  }
}
impl Eq for EntropySignal {}
impl EntropySignal {
  pub fn signal(&self) -> i8 {
    match self {
      EntropySignal::Up => 1,
      EntropySignal::Down => -1,
      EntropySignal::None => 0,
    }
  }
}

/// Based on this blog: https://robotwealth.com/shannon-entropy/
///
/// Translated from Zorro's `ShannonEntropy` indicator, written in C: https://financial-hacker.com/is-scalping-irrational/
///
/// patterns = number of bits + 1.
///
/// If patterns is 3, and the entropy is 3, then the time series is perfectly random.
/// Anything less than the pattern_size means there exists some regularity and therefore predictability.
pub fn shannon_entropy(data: &[f64], length: usize, patterns: usize) -> f64 {
  let mut s = [0u8; 1024]; // hack!
  let size = std::cmp::min(length - patterns - 1, 1024);
  for i in 0..size {
    let mut c = 0;
    for j in 0..patterns {
      if data[i + j] > data[i + j + 1] {
        c += 1 << j;
      }
    }
    s[i] = c as u8;
  }

  let mut hist = [0f64; 256];
  let step = 1.0 / length as f64;
  for &i in s.iter().take(length) {
    hist[i as usize] += step;
  }
  let mut h = 0f64;
  for value in hist {
    if value > 0.0 {
      h -= value * value.log2();
    }
  }
  h
}

pub fn one_step_entropy_signal(series: Dataset, period: usize) -> anyhow::Result<EntropySignal> {
  let mut b1 = series.y().clone();
  let mut b0 = series.y().clone();

  let li = series.len() - 1;
  b1[li] = series.0[li].y + 1.0;
  b0[li] = series.0[li].y - 1.0;

  let length = period + 1;
  let patterns = EntropyBits::One.patterns();
  let e1 = shannon_entropy(&b1, length, patterns);
  let e0 = shannon_entropy(&b0, length, patterns);

  let max = e1.max(e0);
  let up = max == e0 && e0 != e1;
  let down = max == e1 && e0 != e1;

  Ok(if up {
    EntropySignal::Up
  } else if down {
    EntropySignal::Down
  } else {
    EntropySignal::None
  })
}

pub fn shit_test_one_step_entropy_signals(
  series: Dataset,
  period: usize,
) -> anyhow::Result<Vec<(Vec<f64>, EntropySignal)>> {
  let patterns = EntropyBits::One.patterns();

  let mut signals = vec![];
  let mut ups = 0;
  let mut downs = 0;

  let mut last_seen = None;
  let mut second_last_seen = None;
  let y = series.y();
  for i in 0..y.len() - period + 1 {
    let data = y[i..i + period].to_vec();

    if last_seen.is_some() {
      if second_last_seen.is_none() {
        println!("#2 second seen at {}: {:?}", i, data.clone());
      }
      second_last_seen = last_seen.clone();
    }

    if last_seen.is_none() {
      println!("#2 first seen at {}: {:?}", i, data);
    }

    last_seen = Some(data.clone());

    let mut b1 = data.clone();
    let mut b0 = data.clone();

    let li = data.len() - 1;
    b1[li] = data[li] + 1.0;
    b0[li] = data[li] - 1.0;

    let length = period + 1;
    let e1 = shannon_entropy(&b1, length, patterns);
    let e0 = shannon_entropy(&b0, length, patterns);

    let max = e1.max(e0);
    let up = max == e0 && e0 != e1;
    let down = max == e1 && e0 != e1;

    if up {
      signals.push((data, EntropySignal::Up));
      ups += 1;
    } else if down {
      signals.push((data, EntropySignal::Down));
      downs += 1;
    } else {
      signals.push((data, EntropySignal::None));
    }
  }

  println!(
    "#2 {}% were up",
    trunc!(ups as f64 / (ups + downs) as f64 * 100.0, 3)
  );

  if let Some(seen) = last_seen {
    println!("#2 last seen: {:?}", seen);
  }
  if let Some(second_last_seen) = second_last_seen {
    println!("#2 second_last_seen: {:?}", second_last_seen);
  }

  Ok(signals)
}

pub fn two_step_entropy_signal(series: Dataset, period: usize) -> anyhow::Result<EntropySignal> {
  let mut b11 = series.y();
  let mut b00 = series.y();
  let mut b10 = series.y();
  let mut b01 = series.y();

  let last_index = series.len() - 1;
  b11[last_index - 1] = series.0[last_index].y + 1.0;
  b11[last_index] = b11[last_index - 1] + 1.0;

  b00[last_index - 1] = series.0[last_index].y - 1.0;
  b00[last_index] = b00[last_index - 1] - 1.0;

  b10[last_index - 1] = series.0[last_index].y + 1.0;
  b10[last_index] = b10[last_index - 1] - 1.0;

  b01[last_index - 1] = series.0[last_index].y - 1.0;
  b01[last_index] = b01[last_index - 1] + 1.0;

  // todo: this should be the period, right?
  let length = period + 1;
  let patterns = EntropyBits::Two.patterns();
  let e11 = shannon_entropy(&b11, length, patterns);
  let e00 = shannon_entropy(&b00, length, patterns);
  let e10 = shannon_entropy(&b10, length, patterns);
  let e01 = shannon_entropy(&b01, length, patterns);

  let max = e11.max(e00).max(e10).max(e01);

  let up = max == e00 && e00 != e11;
  let down = max == e11 && e00 != e11;
  Ok(if up {
    EntropySignal::Up
  } else if down {
    EntropySignal::Down
  } else {
    EntropySignal::None
  })
}

pub fn shit_test_two_step_entropy_signals(
  series: Dataset,
  period: usize,
) -> anyhow::Result<Vec<(Vec<f64>, EntropySignal)>> {
  let patterns = EntropyBits::Two.patterns();

  let mut signals = vec![];

  let mut last_seen = None;
  let mut second_last_seen = None;
  let y = series.y();
  for i in 0..y.len() - period + 1 {
    let data = y[i..i + period].to_vec();

    if last_seen.is_some() {
      if second_last_seen.is_none() {
        println!("#2 second seen at {}: {:?}", i, data.clone());
      }
      second_last_seen = last_seen.clone();
    }

    if last_seen.is_none() {
      println!("#2 first seen at {}: {:?}", i, data);
    }

    last_seen = Some(data.clone());

    let mut b11 = data.clone();
    let mut b00 = data.clone();
    let mut b10 = data.clone();
    let mut b01 = data.clone();

    let li = data.len() - 1;
    b11[li - 1] = data[li] + 1.0;
    b11[li] = b11[li - 1] + 1.0;

    b00[li - 1] = data[li] - 1.0;
    b00[li] = b00[li - 1] - 1.0;

    b10[li - 1] = data[li] + 1.0;
    b10[li] = b10[li - 1] - 1.0;

    b01[li - 1] = data[li] - 1.0;
    b01[li] = b01[li - 1] + 1.0;

    // todo: this should be the period, right?
    let length = period + 1;
    let e11 = shannon_entropy(&b11, length, patterns);
    let e00 = shannon_entropy(&b00, length, patterns);
    let e10 = shannon_entropy(&b10, length, patterns);
    let e01 = shannon_entropy(&b01, length, patterns);

    let max = e11.max(e00).max(e10).max(e01);

    let up = max == e00 && e00 != e11;
    let down = max == e11 && e00 != e11;

    if up {
      signals.push((data, EntropySignal::Up));
    } else if down {
      signals.push((data, EntropySignal::Down));
    } else {
      signals.push((data, EntropySignal::None));
    }
  }

  if let Some(seen) = last_seen {
    println!("#2 last seen: {:?}", seen);
  }
  if let Some(second_last_seen) = second_last_seen {
    println!("#2 second_last_seen: {:?}", second_last_seen);
  }

  Ok(signals)
}

pub fn three_step_entropy_signal(series: Dataset, period: usize) -> anyhow::Result<EntropySignal> {
  let mut b111 = series.y().clone();
  let mut b000 = series.y().clone();
  let mut b110 = series.y().clone();
  let mut b011 = series.y().clone();
  let mut b101 = series.y().clone();
  let mut b010 = series.y().clone();
  let mut b100 = series.y().clone();
  let mut b001 = series.y().clone();

  let li = series.len() - 1;
  b111[li - 2] = series.0[li].y + 1.0;
  b111[li - 1] = b111[li - 2] + 1.0;
  b111[li] = b111[li - 1] + 1.0;

  b000[li - 2] = series.0[li].y - 1.0;
  b000[li - 1] = b000[li - 2] - 1.0;
  b000[li] = b000[li - 1] - 1.0;

  b110[li - 2] = series.0[li].y + 1.0;
  b110[li - 1] = b110[li - 2] + 1.0;
  b110[li] = b110[li - 1] - 1.0;

  b011[li - 2] = series.0[li].y - 1.0;
  b011[li - 1] = b011[li - 2] + 1.0;
  b011[li] = b011[li - 1] + 1.0;

  b101[li - 2] = series.0[li].y + 1.0;
  b101[li - 1] = b101[li - 2] - 1.0;
  b101[li] = b101[li - 1] + 1.0;

  b010[li - 2] = series.0[li].y - 1.0;
  b010[li - 1] = b010[li - 2] + 1.0;
  b010[li] = b010[li - 1] - 1.0;

  b100[li - 2] = series.0[li].y + 1.0;
  b100[li - 1] = b100[li - 2] - 1.0;
  b100[li] = b100[li - 1] - 1.0;

  b001[li - 2] = series.0[li].y - 1.0;
  b001[li - 1] = b001[li - 2] - 1.0;
  b001[li] = b001[li - 1] + 1.0;

  let length = period + 1;
  let patterns = EntropyBits::Three.patterns();
  let e111 = shannon_entropy(&b111, length, patterns);
  let e000 = shannon_entropy(&b000, length, patterns);
  let e110 = shannon_entropy(&b110, length, patterns);
  let e011 = shannon_entropy(&b011, length, patterns);
  let e101 = shannon_entropy(&b101, length, patterns);
  let e010 = shannon_entropy(&b010, length, patterns);
  let e100 = shannon_entropy(&b100, length, patterns);
  let e001 = shannon_entropy(&b001, length, patterns);

  let max = e111
    .max(e000)
    .max(e110)
    .max(e011)
    .max(e101)
    .max(e010)
    .max(e100)
    .max(e001);

  let up = max == e000 && e000 != e111;
  let down = max == e111 && e000 != e111;

  Ok(if up {
    EntropySignal::Up
  } else if down {
    EntropySignal::Down
  } else {
    EntropySignal::None
  })
}

pub fn four_step_entropy_signal(series: Dataset, period: usize) -> anyhow::Result<EntropySignal> {
  let mut b1111 = series.y().clone();
  let mut b0000 = series.y().clone();
  let mut b1101 = series.y().clone();
  let mut b1011 = series.y().clone();
  let mut b0111 = series.y().clone();
  let mut b1110 = series.y().clone();
  let mut b1100 = series.y().clone();
  let mut b0011 = series.y().clone();
  let mut b1001 = series.y().clone();
  let mut b0110 = series.y().clone();
  let mut b1010 = series.y().clone();
  let mut b0101 = series.y().clone();
  let mut b0100 = series.y().clone();
  let mut b0010 = series.y().clone();
  let mut b1000 = series.y().clone();
  let mut b0001 = series.y().clone();

  let li = series.len() - 1;
  let lp = series.0[li].y;
  b1111[li - 3] = lp + 1.0;
  b1111[li - 2] = b1111[li - 3] + 1.0;
  b1111[li - 1] = b1111[li - 2] + 1.0;
  b1111[li] = b1111[li - 1] + 1.0;

  b0000[li - 3] = lp - 1.0;
  b0000[li - 2] = b0000[li - 3] - 1.0;
  b0000[li - 1] = b0000[li - 2] - 1.0;
  b0000[li] = b0000[li - 1] - 1.0;

  b1101[li - 3] = lp + 1.0;
  b1101[li - 2] = b1101[li - 3] + 1.0;
  b1101[li - 1] = b1101[li - 2] - 1.0;
  b1101[li] = b1101[li - 1] + 1.0;

  b1011[li - 3] = lp + 1.0;
  b1011[li - 2] = b1011[li - 3] - 1.0;
  b1011[li - 1] = b1011[li - 2] + 1.0;
  b1011[li] = b1011[li - 1] + 1.0;

  b0111[li - 3] = lp - 1.0;
  b0111[li - 2] = b0111[li - 3] + 1.0;
  b0111[li - 1] = b0111[li - 2] + 1.0;
  b0111[li] = b0111[li - 1] + 1.0;

  b1110[li - 3] = lp + 1.0;
  b1110[li - 2] = b1110[li - 3] + 1.0;
  b1110[li - 1] = b1110[li - 2] + 1.0;
  b1110[li] = b1110[li - 1] - 1.0;

  b1100[li - 3] = lp + 1.0;
  b1100[li - 2] = b1100[li - 3] + 1.0;
  b1100[li - 1] = b1100[li - 2] - 1.0;
  b1100[li] = b1100[li - 1] - 1.0;

  b0011[li - 3] = lp - 1.0;
  b0011[li - 2] = b0011[li - 3] + 1.0;
  b0011[li - 1] = b0011[li - 2] + 1.0;
  b0011[li] = b0011[li - 1] + 1.0;

  b1001[li - 3] = lp + 1.0;
  b1001[li - 2] = b1001[li - 3] - 1.0;
  b1001[li - 1] = b1001[li - 2] + 1.0;
  b1001[li] = b1001[li - 1] + 1.0;

  b0110[li - 3] = lp - 1.0;
  b0110[li - 2] = b0110[li - 3] + 1.0;
  b0110[li - 1] = b0110[li - 2] + 1.0;
  b0110[li] = b0110[li - 1] - 1.0;

  b1010[li - 3] = lp + 1.0;
  b1010[li - 2] = b1010[li - 3] - 1.0;
  b1010[li - 1] = b1010[li - 2] + 1.0;
  b1010[li] = b1010[li - 1] - 1.0;

  b0101[li - 3] = lp - 1.0;
  b0101[li - 2] = b0101[li - 3] + 1.0;
  b0101[li - 1] = b0101[li - 2] - 1.0;
  b0101[li] = b0101[li - 1] + 1.0;

  b0100[li - 3] = lp - 1.0;
  b0100[li - 2] = b0100[li - 3] + 1.0;
  b0100[li - 1] = b0100[li - 2] - 1.0;
  b0100[li] = b0100[li - 1] - 1.0;

  b0010[li - 3] = lp - 1.0;
  b0010[li - 2] = b0010[li - 3] - 1.0;
  b0010[li - 1] = b0010[li - 2] + 1.0;
  b0010[li] = b0010[li - 1] - 1.0;

  b1000[li - 3] = lp + 1.0;
  b1000[li - 2] = b1000[li - 3] - 1.0;
  b1000[li - 1] = b1000[li - 2] - 1.0;
  b1000[li] = b1000[li - 1] - 1.0;

  b0001[li - 3] = lp - 1.0;
  b0001[li - 2] = b0001[li - 3] - 1.0;
  b0001[li - 1] = b0001[li - 2] - 1.0;
  b0001[li] = b0001[li - 1] + 1.0;

  let length = period + 1;
  let patterns = EntropyBits::Four.patterns();
  let e1111 = shannon_entropy(&b1111, length, patterns);
  let e0000 = shannon_entropy(&b0000, length, patterns);
  let e1101 = shannon_entropy(&b1101, length, patterns);
  let e1011 = shannon_entropy(&b1011, length, patterns);
  let e0111 = shannon_entropy(&b0111, length, patterns);
  let e1110 = shannon_entropy(&b1110, length, patterns);
  let e1100 = shannon_entropy(&b1100, length, patterns);
  let e0011 = shannon_entropy(&b0011, length, patterns);
  let e1001 = shannon_entropy(&b1001, length, patterns);
  let e0110 = shannon_entropy(&b0110, length, patterns);
  let e1010 = shannon_entropy(&b1010, length, patterns);
  let e0101 = shannon_entropy(&b0101, length, patterns);
  let e0100 = shannon_entropy(&b0100, length, patterns);
  let e0010 = shannon_entropy(&b0010, length, patterns);
  let e1000 = shannon_entropy(&b1000, length, patterns);
  let e0001 = shannon_entropy(&b0001, length, patterns);

  let max = e1111
    .max(e0000)
    .max(e1110)
    .max(e1101)
    .max(e1011)
    .max(e0111)
    .max(e1110)
    .max(e1100)
    .max(e1001)
    .max(e0011)
    .max(e1010)
    .max(e0101)
    .max(e0110)
    .max(e0100)
    .max(e0010)
    .max(e1000)
    .max(e0001);

  let up = max == e0000 && e0000 != e1111;
  let down = max == e1111 && e0000 != e1111;

  Ok(if up {
    EntropySignal::Up
  } else if down {
    EntropySignal::Down
  } else {
    EntropySignal::None
  })
}
// ==========================================================================================
//                                     Entropy Tests
// ==========================================================================================

#[cfg(test)]
mod tests {
  use crate::*;

  #[test]
  fn shit_test_one_step() -> anyhow::Result<()> {
    let start_time = Time::new(2017, 1, 1, None, None, None);
    let end_time = Time::new(2019, 1, 5, None, None, None);
    let timeframe = "1h";

    let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
    let ticker = "BTC".to_string();
    let btc_series =
      Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

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
    let btc_series =
      Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

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
    use tradestats::metrics::{
      pearson_correlation_coefficient, rolling_correlation, rolling_zscore,
    };

    let start_time = Time::new(2017, 1, 1, None, None, None);
    let end_time = Time::new(2025, 1, 1, None, None, None);
    let timeframe = "1d";

    let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
    let ticker = "BTC".to_string();
    let btc_series =
      Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;
    // let btc_series = Dataset::new(btc_series.0.into_iter().take(1000).collect());

    let period = 100;
    let bits = EntropyBits::Two;

    let mut cache = RingBuffer::new(period + bits.bits(), ticker);

    struct Entry {
      pub price: f64,
      pub delta: f64,
      pub price_zscore: f64,
      pub entropy: f64,
      #[allow(dead_code)]
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
        EntropyBits::Four => four_step_entropy_signal(in_sample, period)?,
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

  #[test]
  fn entropy_one_step() -> anyhow::Result<()> {
    use super::*;
    dotenv::dotenv().ok();

    let clock_start = Time::now();
    let start_time = Time::new(2019, 1, 1, None, None, None);
    let end_time = Time::new(2021, 1, 1, None, None, None);
    let timeframe = "1h";

    let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
    let ticker = "BTC".to_string();
    let btc_series =
      Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

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
      let e1 = shannon_entropy(&b1, length, patterns);
      let e0 = shannon_entropy(&b0, length, patterns);

      let eli = expected.len() - 1;
      let p0 = series[eli];
      let p1 = series[eli - bits];

      let max = e1.max(e0);

      let up = max == e0 && e0 != e1;
      let down = max == e1 && e0 != e1;

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
      "trades: {}, long: {}/{}, win rate: {}%, long WR: {}%, short WR: {}%, pnl: ${}",
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
  fn entropy_two_step() -> anyhow::Result<()> {
    use super::*;
    dotenv::dotenv().ok();
    let clock_start = Time::now();
    let start_time = Time::new(2019, 1, 1, None, None, None);
    let end_time = Time::new(2021, 1, 1, None, None, None);
    let timeframe = "1h";

    let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
    let ticker = "BTC".to_string();
    let btc_series =
      Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

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
      let e11 = shannon_entropy(&b11, length, patterns);
      let e00 = shannon_entropy(&b00, length, patterns);
      let e10 = shannon_entropy(&b10, length, patterns);
      let e01 = shannon_entropy(&b01, length, patterns);

      let eli = expected.len() - 1;
      let p0 = expected[eli];
      let p2 = expected[eli - bits];

      let max = e11.max(e00).max(e10).max(e01);

      let up = max == e00 && e00 != e11;
      let down = max == e11 && e00 != e11;

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
      "trades: {}, long: {}/{}, win rate: {}%, long WR: {}%, short WR: {}%, pnl: ${}",
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
    let start_time = Time::new(2019, 1, 1, None, None, None);
    let end_time = Time::new(2021, 1, 1, None, None, None);
    let timeframe = "1h";

    let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
    let ticker = "BTC".to_string();
    let btc_series =
      Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

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
      let e111 = shannon_entropy(&b111, length, patterns);
      let e000 = shannon_entropy(&b000, length, patterns);
      let e110 = shannon_entropy(&b110, length, patterns);
      let e011 = shannon_entropy(&b011, length, patterns);
      let e101 = shannon_entropy(&b101, length, patterns);
      let e010 = shannon_entropy(&b010, length, patterns);
      let e100 = shannon_entropy(&b100, length, patterns);
      let e001 = shannon_entropy(&b001, length, patterns);

      let eli = expected.len() - 1;
      let p0 = expected[eli];
      let p3 = expected[eli - bits];

      let max = e111
        .max(e000)
        .max(e110)
        .max(e011)
        .max(e101)
        .max(e010)
        .max(e100)
        .max(e001);

      let up = max == e000 && e000 != e111;
      let down = max == e111 && e000 != e111;

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
      "trades: {}, long: {}/{}, win rate: {}%, long WR: {}%, short WR: {}%, pnl: ${}",
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
  fn entropy_four_step() -> anyhow::Result<()> {
    use super::*;
    dotenv::dotenv().ok();
    let clock_start = Time::now();
    let start_time = Time::new(2019, 1, 1, None, None, None);
    let end_time = Time::new(2021, 1, 1, None, None, None);
    let timeframe = "1h";

    let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
    let ticker = "BTC".to_string();
    let btc_series =
      Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

    let period = 100;
    let bits = 4;
    let patterns = 5;

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

      let mut b1111 = trained.clone();
      let mut b0000 = trained.clone();
      let mut b1101 = trained.clone();
      let mut b1011 = trained.clone();
      let mut b0111 = trained.clone();
      let mut b1110 = trained.clone();
      let mut b1100 = trained.clone();
      let mut b0011 = trained.clone();
      let mut b1001 = trained.clone();
      let mut b0110 = trained.clone();
      let mut b1010 = trained.clone();
      let mut b0101 = trained.clone();
      let mut b0100 = trained.clone();
      let mut b0010 = trained.clone();
      let mut b1000 = trained.clone();
      let mut b0001 = trained.clone();

      let li = trained.len() - 1;
      b1111[li - 3] = trained[li] + 1.0;
      b1111[li - 2] = b1111[li - 3] + 1.0;
      b1111[li - 1] = b1111[li - 2] + 1.0;
      b1111[li] = b1111[li - 1] + 1.0;

      b0000[li - 3] = trained[li] - 1.0;
      b0000[li - 2] = b0000[li - 3] - 1.0;
      b0000[li - 1] = b0000[li - 2] - 1.0;
      b0000[li] = b0000[li - 1] - 1.0;

      b1101[li - 3] = trained[li] + 1.0;
      b1101[li - 2] = b1101[li - 3] + 1.0;
      b1101[li - 1] = b1101[li - 2] - 1.0;
      b1101[li] = b1101[li - 1] + 1.0;

      b1011[li - 3] = trained[li] + 1.0;
      b1011[li - 2] = b1011[li - 3] - 1.0;
      b1011[li - 1] = b1011[li - 2] + 1.0;
      b1011[li] = b1011[li - 1] + 1.0;

      b0111[li - 3] = trained[li] - 1.0;
      b0111[li - 2] = b0111[li - 3] + 1.0;
      b0111[li - 1] = b0111[li - 2] + 1.0;
      b0111[li] = b0111[li - 1] + 1.0;

      b1110[li - 3] = trained[li] + 1.0;
      b1110[li - 2] = b1110[li - 3] + 1.0;
      b1110[li - 1] = b1110[li - 2] + 1.0;
      b1110[li] = b1110[li - 1] - 1.0;

      b1100[li - 3] = trained[li] + 1.0;
      b1100[li - 2] = b1100[li - 3] + 1.0;
      b1100[li - 1] = b1100[li - 2] - 1.0;
      b1100[li] = b1100[li - 1] - 1.0;

      b0011[li - 3] = trained[li] - 1.0;
      b0011[li - 2] = b0011[li - 3] + 1.0;
      b0011[li - 1] = b0011[li - 2] + 1.0;
      b0011[li] = b0011[li - 1] + 1.0;

      b1001[li - 3] = trained[li] + 1.0;
      b1001[li - 2] = b1001[li - 3] - 1.0;
      b1001[li - 1] = b1001[li - 2] + 1.0;
      b1001[li] = b1001[li - 1] + 1.0;

      b0110[li - 3] = trained[li] - 1.0;
      b0110[li - 2] = b0110[li - 3] + 1.0;
      b0110[li - 1] = b0110[li - 2] + 1.0;
      b0110[li] = b0110[li - 1] - 1.0;

      b1010[li - 3] = trained[li] + 1.0;
      b1010[li - 2] = b1010[li - 3] - 1.0;
      b1010[li - 1] = b1010[li - 2] + 1.0;
      b1010[li] = b1010[li - 1] - 1.0;

      b0101[li - 3] = trained[li] - 1.0;
      b0101[li - 2] = b0101[li - 3] + 1.0;
      b0101[li - 1] = b0101[li - 2] - 1.0;
      b0101[li] = b0101[li - 1] + 1.0;

      b0100[li - 3] = trained[li] - 1.0;
      b0100[li - 2] = b0100[li - 3] + 1.0;
      b0100[li - 1] = b0100[li - 2] - 1.0;
      b0100[li] = b0100[li - 1] - 1.0;

      b0010[li - 3] = trained[li] - 1.0;
      b0010[li - 2] = b0010[li - 3] - 1.0;
      b0010[li - 1] = b0010[li - 2] + 1.0;
      b0010[li] = b0010[li - 1] - 1.0;

      b1000[li - 3] = trained[li] + 1.0;
      b1000[li - 2] = b1000[li - 3] - 1.0;
      b1000[li - 1] = b1000[li - 2] - 1.0;
      b1000[li] = b1000[li - 1] - 1.0;

      b0001[li - 3] = trained[li] - 1.0;
      b0001[li - 2] = b0001[li - 3] - 1.0;
      b0001[li - 1] = b0001[li - 2] - 1.0;
      b0001[li] = b0001[li - 1] + 1.0;

      let length = period + 1;
      let e1111 = shannon_entropy(&b1111, length, patterns);
      let e0000 = shannon_entropy(&b0000, length, patterns);
      let e1101 = shannon_entropy(&b1101, length, patterns);
      let e1011 = shannon_entropy(&b1011, length, patterns);
      let e0111 = shannon_entropy(&b0111, length, patterns);
      let e1110 = shannon_entropy(&b1110, length, patterns);
      let e1100 = shannon_entropy(&b1100, length, patterns);
      let e0011 = shannon_entropy(&b0011, length, patterns);
      let e1001 = shannon_entropy(&b1001, length, patterns);
      let e0110 = shannon_entropy(&b0110, length, patterns);
      let e1010 = shannon_entropy(&b1010, length, patterns);
      let e0101 = shannon_entropy(&b0101, length, patterns);
      let e0100 = shannon_entropy(&b0100, length, patterns);
      let e0010 = shannon_entropy(&b0010, length, patterns);
      let e1000 = shannon_entropy(&b1000, length, patterns);
      let e0001 = shannon_entropy(&b0001, length, patterns);

      let eli = expected.len() - 1;
      let p0 = expected[eli];
      let p4 = expected[eli - bits];

      let max = e1111
        .max(e0000)
        .max(e1110)
        .max(e1101)
        .max(e1011)
        .max(e0111)
        .max(e1110)
        .max(e1100)
        .max(e1001)
        .max(e0011)
        .max(e1010)
        .max(e0101)
        .max(e0110)
        .max(e0100)
        .max(e0010)
        .max(e1000)
        .max(e0001);

      let up = max == e0000 && e0000 != e1111;
      let down = max == e1111 && e0000 != e1111;

      if up {
        longs += 1;
        if p4 < p0 {
          // long && new price > old price, long wins
          long_win += 1;
          cum_pnl += p0 - p4;
        } else {
          // long && new price < old price, long loses
          cum_pnl += p0 - p4;
        }
      } else if down {
        shorts += 1;
        if p4 > p0 {
          // short && old price > new price, short wins
          short_win += 1;
          cum_pnl += p4 - p0;
        } else {
          // short && old price < new price, short loses
          cum_pnl += p4 - p0;
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
      "trades: {}, long: {}/{}, win rate: {}%, long WR: {}%, short WR: {}%, pnl: ${}",
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
  fn entropy_n_step() -> anyhow::Result<()> {
    use super::*;
    dotenv::dotenv().ok();
    init_logger();

    let clock_start = Time::now();
    let start_time = Time::new(2019, 1, 1, None, None, None);
    let end_time = Time::new(2021, 1, 1, None, None, None);
    let timeframe = "1h";

    let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
    let ticker = "BTC".to_string();
    let btc_series =
      Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

    let period = 100;
    let bits = 1;
    let patterns = bits + 1;

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

      let eli = expected.len() - 1;
      let p0 = expected[eli];
      let pn = expected[eli - bits];

      let signal = n_bit_entropy!(bits, period, trained)?;

      let up = matches!(signal, EntropySignal::Up);
      let down = matches!(signal, EntropySignal::Down);

      if up {
        longs += 1;
        if pn < p0 {
          // long && new price > old price, long wins
          long_win += 1;
          cum_pnl += p0 - pn;
        } else {
          // long && new price < old price, long loses
          cum_pnl += p0 - pn;
        }
      } else if down {
        shorts += 1;
        if pn > p0 {
          // short && old price > new price, short wins
          short_win += 1;
          cum_pnl += pn - p0;
        } else {
          // short && old price < new price, short loses
          cum_pnl += pn - p0;
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
      "trades: {}, long: {}/{}, win rate: {}%, long WR: {}%, short WR: {}%, pnl: ${}",
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
}
