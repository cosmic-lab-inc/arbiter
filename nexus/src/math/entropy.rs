use crate::Dataset;

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

#[derive(Debug, Clone)]
pub enum EntropySignal {
  Up,
  Down,
  None,
}
impl PartialEq for EntropySignal {
  fn eq(&self, other: &Self) -> bool {
    match (self, other) {
      (EntropySignal::Up, EntropySignal::Up) => true,
      (EntropySignal::Down, EntropySignal::Down) => true,
      (EntropySignal::None, EntropySignal::None) => true,
      _ => false,
    }
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

  b1[0] = series.0[0].y + 1.0;
  b0[0] = series.0[0].y - 1.0;

  let length = period + 1;
  let patterns = EntropyBits::One.patterns();
  let entropy_b1 = shannon_entropy(&b1, length, patterns);
  let entropy_b0 = shannon_entropy(&b0, length, patterns);

  let last_index = series.len() - 1;
  let p0 = &series.0[last_index];
  let p1 = &series.0[last_index - EntropyBits::One.bits()];

  let max = entropy_b1.max(entropy_b0);

  // original
  Ok(if max == entropy_b1 && p1.y > p0.y {
    EntropySignal::Up
  } else if max == entropy_b0 && p1.y < p0.y {
    EntropySignal::Down
  } else {
    EntropySignal::None
  })
}

pub fn two_step_entropy_signal(series: Dataset, period: usize) -> anyhow::Result<EntropySignal> {
  let mut b11 = series.y();
  let mut b00 = series.y();
  let mut b10 = series.y();
  let mut b01 = series.y();

  b11[1] = series.0[0].y + 1.0;
  b11[0] = b11[1] + 1.0;

  b00[1] = series.0[0].y - 1.0;
  b00[0] = b00[1] - 1.0;

  b10[1] = series.0[0].y + 1.0;
  b10[0] = b10[1] - 1.0;

  b01[1] = series.0[0].y - 1.0;
  b01[0] = b01[1] + 1.0;

  // todo: this should be the period, right?
  let length = period + 1;
  let patterns = EntropyBits::Two.patterns();
  let entropy_b11 = shannon_entropy(&b11, length, patterns);
  let entropy_b00 = shannon_entropy(&b00, length, patterns);
  let entropy_b10 = shannon_entropy(&b10, length, patterns);
  let entropy_b01 = shannon_entropy(&b01, length, patterns);

  let last_index = series.len() - 1;
  let p0 = &series.0[last_index];
  let p2 = &series.0[last_index - EntropyBits::Two.bits()];

  let max = entropy_b11
    .max(entropy_b00)
    .max(entropy_b10)
    .max(entropy_b01);

  Ok(if max == entropy_b11 && p2.y > p0.y {
    EntropySignal::Up
  } else if max == entropy_b00 && p2.y < p0.y {
    EntropySignal::Down
  } else {
    EntropySignal::None
  })
}

pub fn _two_step_entropy_signals(
  series: Dataset,
  period: usize,
) -> anyhow::Result<Vec<(Vec<f64>, EntropySignal)>> {
  let bits = EntropyBits::Two.bits();
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

    b11[1] = data[0] + 1.0;
    b11[0] = b11[1] + 1.0;

    b00[1] = data[0] - 1.0;
    b00[0] = b00[1] - 1.0;

    b10[1] = data[0] + 1.0;
    b10[0] = b10[1] - 1.0;

    b01[1] = data[0] - 1.0;
    b01[0] = b01[1] + 1.0;

    // todo: this should be the period, right?
    let length = period + 1;
    let entropy_b11 = shannon_entropy(&b11, length, patterns);
    let entropy_b00 = shannon_entropy(&b00, length, patterns);
    let entropy_b10 = shannon_entropy(&b10, length, patterns);
    let entropy_b01 = shannon_entropy(&b01, length, patterns);

    let last_index = data.len() - 1;
    let p0 = data[last_index];
    let p2 = data[last_index - bits];

    let max = entropy_b11
      .max(entropy_b00)
      .max(entropy_b10)
      .max(entropy_b01);

    if max == entropy_b11 && p2 > p0 {
      signals.push((data, EntropySignal::Up));
    } else if max == entropy_b00 && p2 < p0 {
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

  b111[2] = series.0[0].y + 1.0;
  b111[1] = b111[2] + 1.0;
  b111[0] = b111[1] + 1.0;

  b000[2] = series.0[0].y - 1.0;
  b000[1] = b000[2] - 1.0;
  b000[0] = b000[1] - 1.0;

  b110[2] = series.0[0].y + 1.0;
  b110[1] = b110[2] + 1.0;
  b110[0] = b110[1] - 1.0;

  b011[2] = series.0[0].y - 1.0;
  b011[1] = b011[2] + 1.0;
  b011[0] = b011[1] + 1.0;

  b101[2] = series.0[0].y + 1.0;
  b101[1] = b101[2] - 1.0;
  b101[0] = b101[1] + 1.0;

  b010[2] = series.0[0].y - 1.0;
  b010[1] = b010[2] + 1.0;
  b010[0] = b010[1] - 1.0;

  b100[2] = series.0[0].y + 1.0;
  b100[1] = b100[2] - 1.0;
  b100[0] = b100[1] - 1.0;

  b001[2] = series.0[0].y - 1.0;
  b001[1] = b001[2] - 1.0;
  b001[0] = b001[1] + 1.0;

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

  let last_index = series.len() - 1;
  let p0 = &series.0[last_index];
  let p3 = &series.0[last_index - EntropyBits::Three.bits()];

  let max = entropy_b111
    .max(entropy_b000)
    .max(entropy_b110)
    .max(entropy_b011)
    .max(entropy_b101)
    .max(entropy_b010)
    .max(entropy_b100)
    .max(entropy_b001);

  // original
  Ok(if max == entropy_b111 && p3.y > p0.y {
    EntropySignal::Up
  } else if max == entropy_b000 && p3.y < p0.y {
    EntropySignal::Down
  } else {
    EntropySignal::None
  })
}
