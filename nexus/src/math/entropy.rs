use crate::Dataset;

pub enum EntropySignal {
  Up,
  Down,
  None,
}

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

pub fn shannon_entropy_signal(
  series: Dataset,
  period: usize,
  patterns: usize,
) -> anyhow::Result<EntropySignal> {
  let mut b11 = series.y().clone();
  let mut b00 = series.y().clone();
  let mut b10 = series.y().clone();
  let mut b01 = series.y().clone();

  b11[1] = series.0[0].y + 1.0;
  b11[0] = b11[1] + 1.0;

  b00[1] = series.0[0].y - 1.0;
  b00[0] = b00[1] - 1.0;

  b10[1] = series.0[0].y + 1.0;
  b10[0] = b10[1] - 1.0;

  b01[1] = series.0[0].y - 1.0;
  b01[0] = b01[1] + 1.0;

  let entropy_b11 = shannon_entropy(&b11, period + 1, patterns);
  let entropy_b00 = shannon_entropy(&b00, period + 1, patterns);
  let entropy_b10 = shannon_entropy(&b10, period + 1, patterns);
  let entropy_b01 = shannon_entropy(&b01, period + 1, patterns);

  let last_index = series.len() - 1;
  let p0 = &series.0[last_index];
  let p2 = &series.0[last_index - 2];

  let max = entropy_b11
    .max(entropy_b00)
    .max(entropy_b10)
    .max(entropy_b01);

  // original
  Ok(if max == entropy_b11 && p2.y > p0.y {
    EntropySignal::Up
  } else if max == entropy_b00 && p2.y < p0.y {
    EntropySignal::Down
  } else {
    EntropySignal::None
  })
}
