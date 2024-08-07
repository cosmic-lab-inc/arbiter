/// Corrected R over S Hurst exponent
/// Hurst <0.5: mean-reverting
/// Hurst =0.5: random walk
/// Hurst >0.5: trending
pub fn hurst(x: Vec<f64>) -> f64 {
  let mut cap_x: Vec<f64> = vec![x.len() as f64];
  let mut cap_y: Vec<f64> = vec![rscalc(&x)];
  let mut n: Vec<u64> = vec![0, x.len() as u64 / 2, x.len() as u64];
  // compute averaged R/S for halved intervals
  while n[1] >= 8  {
    let mut xl: Vec<f64> = vec![];
    let mut yl: Vec<f64> = vec![];
    for i in 1..n.len() {
      let rs: f64 = rscalc(&x[((n[i-1]+1) as usize)..(n[i] as usize)]);
      xl.push((n[i]-n[i-1]) as f64);
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
  let result: Vec<f64> = x.iter()
  .scan(0f64, |acc, &a| {
    *acc += a;
    Some(*acc)
  }).collect();
  result
}

pub fn minmax(x: &[f64]) -> (f64, f64) {
  return x.iter().fold((x[0], x[0]), |acc, &x| (acc.0.min(x), acc.1.max(x)));
}

/// define the R/S scale
pub fn rscalc(x: &[f64]) -> f64 {
  let x_mean: f64 = mean(x);
  let x_minus_mean: Vec<f64> = x.iter()
    .map(|x| x - x_mean)
    .collect();
  let y: Vec<f64> = cumsum(&x_minus_mean);
  let (min_y, max_y) = minmax(&y);
  let r: f64 = (max_y - min_y).abs();
  let s: f64 = std_dev(x);
  let result: f64 =  r / s;
  result
}

// half intervals of indices
pub fn half(n: &[u64], original_length: u64) -> Vec<u64> {
  let previous_step: u64 =  n[1];
  let next_step: u64 = previous_step / 2;
  let length: u64 = original_length / next_step;
  let range: Vec<u64> = (0..length + 1).collect();
  let result: Vec<u64> = range.iter().map(|a| a * next_step).collect();
  result
}