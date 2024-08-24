use crate::{trunc, Dataset};

#[derive(Debug, Copy, Clone)]
pub enum EntropyBits {
  One,
  Two,
  Three,
}
impl EntropyBits {
  pub fn bits(&self) -> usize {
    match self {
      EntropyBits::One => 1,
      EntropyBits::Two => 2,
      EntropyBits::Three => 3,
    }
  }

  pub fn patterns(&self) -> usize {
    match self {
      EntropyBits::One => 2,
      EntropyBits::Two => 3,
      EntropyBits::Three => 4,
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

/// Based on this blog: https://robotwealth.com/shannon-entropy/
/// Translated from Zorro's `ShannonEntropy` indicator, written in C: https://financial-hacker.com/is-scalping-irrational/
/// pattern_size = number of bits. If bits is 3, and the result is 3, then the time series is perfectly random.
/// Anything less than the pattern_size means there exists some regularity and therefore predictability.
pub fn shannon_entropy(data: &[f64], length: usize, pattern_size: usize) -> f64 {
  let mut s = [0u8; 1024]; // hack!
  let size = std::cmp::min(length - pattern_size - 1, 1024);
  for i in 0..size {
    let mut c = 0;
    for j in 0..pattern_size {
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
  let entropy_b1 = shannon_entropy(&b1, length, patterns);
  let entropy_b0 = shannon_entropy(&b0, length, patterns);

  let max = entropy_b1.max(entropy_b0);
  let up = max == entropy_b1;
  let down = max == entropy_b0;

  // let up = entropy_b1 > entropy_b0;
  // let down = entropy_b0 > entropy_b1;

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
    let entropy_b1 = shannon_entropy(&b1, length, patterns);
    let entropy_b0 = shannon_entropy(&b0, length, patterns);

    let max = entropy_b1.max(entropy_b0);
    let up = max == entropy_b1;
    let down = max == entropy_b0;

    // let up = entropy_b1 > entropy_b0;
    // let down = entropy_b0 > entropy_b1;

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
  let entropy_b11 = shannon_entropy(&b11, length, patterns);
  let entropy_b00 = shannon_entropy(&b00, length, patterns);
  let entropy_b10 = shannon_entropy(&b10, length, patterns);
  let entropy_b01 = shannon_entropy(&b01, length, patterns);

  let max = entropy_b11
    .max(entropy_b00)
    .max(entropy_b10)
    .max(entropy_b01);

  let up = max == entropy_b11;
  let down = max == entropy_b00;
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
    let entropy_b11 = shannon_entropy(&b11, length, patterns);
    let entropy_b00 = shannon_entropy(&b00, length, patterns);
    let entropy_b10 = shannon_entropy(&b10, length, patterns);
    let entropy_b01 = shannon_entropy(&b01, length, patterns);

    let max = entropy_b11
      .max(entropy_b00)
      .max(entropy_b10)
      .max(entropy_b01);

    let up = max == entropy_b11;
    let down = max == entropy_b00;

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
  let entropy_b111 = shannon_entropy(&b111, length, patterns);
  let entropy_b000 = shannon_entropy(&b000, length, patterns);
  let entropy_b110 = shannon_entropy(&b110, length, patterns);
  let entropy_b011 = shannon_entropy(&b011, length, patterns);
  let entropy_b101 = shannon_entropy(&b101, length, patterns);
  let entropy_b010 = shannon_entropy(&b010, length, patterns);
  let entropy_b100 = shannon_entropy(&b100, length, patterns);
  let entropy_b001 = shannon_entropy(&b001, length, patterns);

  let max = entropy_b111
    .max(entropy_b000)
    .max(entropy_b110)
    .max(entropy_b011)
    .max(entropy_b101)
    .max(entropy_b010)
    .max(entropy_b100)
    .max(entropy_b001);

  // original
  Ok(if max == entropy_b111 {
    EntropySignal::Up
  } else if max == entropy_b000 {
    EntropySignal::Down
  } else {
    EntropySignal::None
  })
}
