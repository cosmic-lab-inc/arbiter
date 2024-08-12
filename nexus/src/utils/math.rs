use log::warn;

/// Corrected R over S Hurst exponent
/// Hurst <0.5: mean-reverting
/// Hurst =0.5: random walk
/// Hurst >0.5: trending
pub fn hurst(x: Vec<f64>) -> f64 {
  let mut cap_x: Vec<f64> = vec![x.len() as f64];
  let mut cap_y: Vec<f64> = vec![rscalc(&x)];
  let mut n: Vec<u64> = vec![0, x.len() as u64 / 2, x.len() as u64];
  // compute averaged R/S for halved intervals
  while n[1] >= 8 {
    let mut xl: Vec<f64> = vec![];
    let mut yl: Vec<f64> = vec![];
    for i in 1..n.len() {
      let rs: f64 = rscalc(&x[((n[i - 1] + 1) as usize)..(n[i] as usize)]);
      xl.push((n[i] - n[i - 1]) as f64);
      yl.push(rs);
    }
    cap_x.push(mean(&xl));
    cap_y.push(mean(&yl));
    // next step
    n = half(&n, x.len() as u64);
  }
  // apply linear regression
  let cap_x_log: Vec<f64> = cap_x.iter().map(|a| a.ln()).collect();
  let cap_y_log: Vec<f64> = cap_y.iter().map(|a| a.ln()).collect();
  let (slope, _): (f64, f64) = linreg::linear_regression(&cap_x_log, &cap_y_log).unwrap();
  slope
}

pub fn mean(x: &[f64]) -> f64 {
  let sum: f64 = x.iter().sum();
  let n: f64 = x.len() as f64;
  sum / n
}

pub fn std_dev(x: &[f64]) -> f64 {
  let mean_x: f64 = mean(x);
  let sum_x_minus_mean: f64 = x.iter().map(|a| (a - mean_x).powi(2)).sum();
  (sum_x_minus_mean / (x.len() as f64)).sqrt()
}

pub fn cumsum(x: &[f64]) -> Vec<f64> {
  let result: Vec<f64> = x
    .iter()
    .scan(0f64, |acc, &a| {
      *acc += a;
      Some(*acc)
    })
    .collect();
  result
}

pub fn minmax(x: &[f64]) -> (f64, f64) {
  return x
    .iter()
    .fold((x[0], x[0]), |acc, &x| (acc.0.min(x), acc.1.max(x)));
}

/// define the R/S scale
pub fn rscalc(x: &[f64]) -> f64 {
  let x_mean: f64 = mean(x);
  let x_minus_mean: Vec<f64> = x.iter().map(|x| x - x_mean).collect();
  let y: Vec<f64> = cumsum(&x_minus_mean);
  let (min_y, max_y) = minmax(&y);
  let r: f64 = (max_y - min_y).abs();
  let s: f64 = std_dev(x);
  let result: f64 = r / s;
  result
}

// half intervals of indices
pub fn half(n: &[u64], original_length: u64) -> Vec<u64> {
  let previous_step: u64 = n[1];
  let next_step: u64 = previous_step / 2;
  let length: u64 = original_length / next_step;
  let range: Vec<u64> = (0..length + 1).collect();
  let result: Vec<u64> = range.iter().map(|a| a * next_step).collect();
  result
}

/// ZScore of last index in a spread time series
pub fn zscore(series: &[f64], window: usize) -> anyhow::Result<f64> {
  // Guard: Ensure correct window size
  if window > series.len() {
    return Err(anyhow::anyhow!("Window size is greater than vector length"));
  }

  // last z score
  let window_data: &[f64] = &series[series.len() - window..];
  let mean: f64 = window_data.iter().sum::<f64>() / window_data.len() as f64;
  let var: f64 = window_data
    .iter()
    .map(|&val| (val - mean).powi(2))
    .sum::<f64>()
    / (window_data.len() - 1) as f64;
  let std_dev: f64 = var.sqrt();
  if std_dev == 0.0 {
    warn!(
      "Standard deviation is zero with var {}, mean {}, and len {}",
      var,
      mean,
      window_data.len()
    );
    return Ok(0.0);
  }
  let z_score = (series[series.len() - 1] - mean) / std_dev;
  Ok(z_score)
}

/// Translate from Zorro's `ShannonEntropy` indicator, written in C: https://financial-hacker.com/is-scalping-irrational/
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
  inner_shannon_entropy(&s, size)
}
fn inner_shannon_entropy(s: &[u8], length: usize) -> f64 {
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
