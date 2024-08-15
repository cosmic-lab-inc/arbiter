use crate::{Data, Dataset, Freq, Signals, FFT};
use log::warn;
use ndarray::Array1;
use rustfft::num_complex::Complex;
use rustfft::num_traits::Zero;
use rustfft::FftPlanner;
use std::cmp::Ordering;

pub fn std_dev(x: &[f64]) -> f64 {
  let mean_x: f64 = mean(x);
  let sum_x_minus_mean: f64 = x.iter().map(|a| (a - mean_x).powi(2)).sum();
  (sum_x_minus_mean / (x.len() as f64)).sqrt()
}

pub fn mean(x: &[f64]) -> f64 {
  let sum: f64 = x.iter().sum();
  let n: f64 = x.len() as f64;
  sum / n
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
