use crate::{Data, Dataset};
use ndarray::Array1;
use rustfft::num_complex::Complex;
use rustfft::num_traits::Zero;
use rustfft::FftPlanner;
use std::cmp::Ordering;
use std::f64::consts::PI;

pub struct Freq {
  pub mag: f64,
  pub period: f64,
}

pub struct FFT {
  pub trained: Dataset,
  pub filtered: Dataset,
  pub predicted: Option<Dataset>,
}

fn fft_frequencies(n: usize, d: f64) -> Vec<f64> {
  let val = 1.0 / (n as f64 * d);
  let mut result = Vec::with_capacity(n);
  let m = if n % 2 == 0 { n / 2 } else { n / 2 + 1 };
  for i in 0..m {
    result.push(i as f64 * val);
  }
  for i in -(n as i64 / 2)..0 {
    result.push(i as f64 * val);
  }
  result
}

/// Reference: https://medium.com/@kt.26karanthakur/stock-market-signal-analysis-using-fast-fourier-transform-e3bdde7bcee6
/// Research: https://www.math.utah.edu/~gustafso/s2017/2270/projects-2016/williamsBarrett/williamsBarrett-Fast-Fourier-Transform-Predicting-Financial-Securities-Prices.pdf
pub fn fft(series: Dataset, dominant_freq_cutoff: usize) -> anyhow::Result<FFT> {
  // Assuming df['Close'] is a Vec<f64>
  let close_prices: Vec<f64> = series.y();

  let fft_len = close_prices.len();
  let mut planner = FftPlanner::new();
  let fft = planner.plan_fft_forward(fft_len);

  //
  // Convert to FFT frequencies
  //

  let mut fft_input: Vec<Complex<f64>> = close_prices
    .into_iter()
    .map(|x| Complex::new(x, 0.0))
    .collect();
  // Perform FFT
  fft.process(&mut fft_input);

  // Calculate FFT frequencies
  let sample_spacing = 1.0; // Assuming daily data, d=1
  let frequencies: Vec<f64> = fft_frequencies(fft_len, sample_spacing);
  // Calculate magnitude
  let magnitude: Vec<f64> = fft_input.iter().map(|x| x.norm()).collect();
  // Calculate periods
  let periods: Array1<f64> = 1.0 / Array1::from(frequencies.clone());

  //
  // Reconstruct time series from FFT frequencies (inverse FFT)
  //

  let mut ifft_input = fft_input.clone();
  let ifft = planner.plan_fft_inverse(fft_len);
  ifft.process(&mut ifft_input);

  // The input vector now contains the IFFT result, which should be close to the original time series
  let original_input: Vec<f64> = ifft_input.iter().map(|x| x.re).collect();
  let original_data: Vec<Data> = original_input
    .iter()
    .enumerate()
    .map(|(i, &y)| Data { x: i as i64, y })
    .collect();

  //
  // Reconstruct time series from dominant (top 25) FFT frequencies
  //

  let mut top_ifft_input = fft_input.clone();
  let top_ifft = planner.plan_fft_inverse(fft_len);

  let mut sorted_freq: Vec<Freq> = periods
    .iter()
    .zip(magnitude.iter())
    .map(|(&x, &y)| Freq { mag: y, period: x }) // Swap to sort by magnitude
    .collect();

  // Sort by magnitude in descending order and take the top 25
  sorted_freq.sort_by(|a, b| b.period.partial_cmp(&a.period).unwrap_or(Ordering::Equal));

  let dominant_periods: Vec<f64> = sorted_freq
    .into_iter()
    .map(|freq| freq.period)
    .take(dominant_freq_cutoff)
    .collect();

  // Find the minimum period of the top 25
  let min_period = *dominant_periods
    .iter()
    .min_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal))
    .unwrap();

  // Set the values to zero where the absolute value of the frequencies is greater than the inverse of the minimum of the top periods
  for (i, &freq) in frequencies.iter().enumerate() {
    if freq.abs() > 1.0 / min_period {
      top_ifft_input[i] = Complex::zero();
    }
  }

  top_ifft.process(&mut top_ifft_input);

  // The vector now contains the IFFT result of the top 25 (dominant) periods
  let filtered_input: Vec<f64> = top_ifft_input
    .iter()
    .map(|x| x.re / fft_len as f64)
    .collect();
  let filtered_data: Vec<Data> = filtered_input
    .iter()
    .enumerate()
    .map(|(i, &y)| Data { x: i as i64, y })
    .collect();

  Ok(FFT {
    trained: Dataset::new(original_data),
    filtered: Dataset::new(filtered_data),
    predicted: None,
  })
}

pub fn ifft(series: Dataset) -> anyhow::Result<FFT> {
  // Assuming df['Close'] is a Vec<f64>
  let close_prices: Vec<f64> = series.y();

  let mut fft_input: Vec<Complex<f64>> = close_prices
    .into_iter()
    .map(|x| Complex::new(x, 0.0))
    .collect();

  //
  // Convert to FFT frequencies
  //
  let fft_len = fft_input.len();
  let mut planner = FftPlanner::new();
  let fft = planner.plan_fft_forward(fft_len);
  fft.process(&mut fft_input);

  //
  // Reconstruct time series from FFT frequencies (inverse FFT)
  //
  let ifft = planner.plan_fft_inverse(fft_len);
  ifft.process(&mut fft_input);

  // The input vector now contains the IFFT result, which should be close to the original time series
  let ifft_series: Vec<f64> = fft_input.iter().map(|x| x.re / fft_len as f64).collect();
  let ifft_data: Vec<Data> = ifft_series
    .iter()
    .enumerate()
    .map(|(i, &y)| Data { x: i as i64, y })
    .collect();

  let original = series
    .clone()
    .0
    .into_iter()
    .enumerate()
    .map(|(i, d)| Data {
      x: i as i64,
      y: d.y,
    })
    .collect();

  Ok(FFT {
    trained: Dataset::new(original),
    filtered: Dataset::new(ifft_data),
    predicted: None,
  })
}

pub fn dft(
  series: Dataset,
  dominant_freq_cutoff: usize,
  interpolate: usize,
) -> anyhow::Result<FFT> {
  let mut fft_input: Vec<Complex<f64>> = series
    .y()
    .into_iter()
    .map(|x| Complex::new(x, 0.0))
    .collect();

  let fft_len = fft_input.len();
  let mut planner = FftPlanner::new();

  //
  // Convert to FFT frequencies
  //
  let fft = planner.plan_fft_forward(fft_len);
  fft.process(&mut fft_input);

  // Calculate FFT frequencies
  let sample_spacing = 1.0; // Assuming daily data, d=1
  let frequencies: Vec<f64> = fft_frequencies(fft_len, sample_spacing);
  // Calculate magnitude
  let magnitude: Vec<f64> = fft_input.iter().map(|x| x.norm()).collect();
  // Calculate periods
  let periods: Array1<f64> = 1.0 / Array1::from(frequencies.clone());

  //
  // Reconstruct time series from FFT frequencies (inverse FFT)
  //

  let mut ifft_input = fft_input.clone();
  let mut ifft_planner = FftPlanner::new();
  let ifft = ifft_planner.plan_fft_inverse(fft_len);
  ifft.process(&mut ifft_input);

  // The input vector now contains the IFFT result, which should be close to the original time series
  let original_input: Vec<f64> = ifft_input.iter().map(|x| x.re / fft_len as f64).collect();
  let original_data: Vec<Data> = original_input
    .iter()
    .enumerate()
    .map(|(i, &y)| Data { x: i as i64, y })
    .collect();

  //
  // Reconstruct time series from dominant FFT frequencies
  //

  let mut top_ifft_input = fft_input.clone();
  let top_ifft = planner.plan_fft_inverse(fft_len);

  let mut sorted_freq: Vec<Freq> = periods
    .iter()
    .zip(magnitude.iter())
    .map(|(&x, &y)| Freq { mag: y, period: x }) // Swap to sort by magnitude
    .collect();

  // Sort by magnitude in descending order and take the top 25
  sorted_freq.sort_by(|a, b| b.period.partial_cmp(&a.period).unwrap_or(Ordering::Equal));

  let dominant_periods: Vec<f64> = sorted_freq
    .into_iter()
    .map(|freq| freq.period)
    .take(dominant_freq_cutoff)
    .collect();

  // Find the minimum period of the top 25
  let min_period = *dominant_periods
    .iter()
    .min_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal))
    .unwrap();

  // Set the values to zero where the absolute value of the frequencies is greater than the inverse of the minimum of the top periods
  for (i, &freq) in frequencies.iter().enumerate() {
    if freq.abs() > 1.0 / min_period {
      top_ifft_input[i] = Complex::zero();
    }
  }

  top_ifft.process(&mut top_ifft_input);

  // The vector now contains the IFFT result of the top 25 (dominant) periods
  let filtered_input: Vec<f64> = top_ifft_input
    .iter()
    .map(|x| x.re / fft_len as f64)
    .collect();
  let filtered_data: Vec<Data> = filtered_input
    .iter()
    .enumerate()
    .map(|(i, &y)| Data { x: i as i64, y })
    .collect();

  //
  // Extrapolate the next N points using Discrete Fourier Transform (DFT)
  //

  // // Method #2
  let filtered_input_array: Array1<f64> = Array1::from(filtered_input.clone());

  // Use the fourier_extrapolation function to predict the next interpolate points
  let predicted: Vec<f64> =
    fft_extrap(filtered_input_array, interpolate, dominant_freq_cutoff).to_vec();

  // let extrap = predicted[predicted.len() - interpolate..].to_vec();
  // // Convert the predicted_data_array back into a Vec<Data>
  // let predicted_data: Vec<Data> = extrap
  //   .iter()
  //   .enumerate()
  //   .map(|(i, &y)| Data {
  //     x: (fft_len + i) as i64,
  //     y,
  //   })
  //   .collect();

  // Convert the predicted_data_array back into a Vec<Data>
  let predicted_data: Vec<Data> = predicted
    .iter()
    .enumerate()
    .map(|(i, &y)| Data { x: i as i64, y })
    .collect();

  Ok(FFT {
    trained: Dataset::new(original_data),
    filtered: Dataset::new(filtered_data),
    predicted: Some(Dataset::new(predicted_data)),
  })
}

fn fft_extrap(x: Array1<f64>, n_predict: usize, frequencies: usize) -> Array1<f64> {
  let n = x.len();
  let n_harm = frequencies; // number of harmonics in model
  let t: Array1<f64> = Array1::range(0.0, n as f64, 1.0);
  let input = t.clone().into_iter().zip(x.clone()).collect::<Vec<_>>();
  let (slope, intercept) = linreg::linear_regression_of::<f64, f64, f64>(input.as_slice()).unwrap();
  let x_notrend = x - &(slope * &t + intercept); // detrended x

  // detrended x in frequency domain
  let mut planner = FftPlanner::<f64>::new();
  let fft = planner.plan_fft_forward(n);
  let mut x_freqdom: Vec<Complex<f64>> = x_notrend.mapv(|x| Complex::new(x, 0.0)).to_vec();
  fft.process(&mut x_freqdom);

  // frequencies
  let f: Vec<f64> = (0..n).map(|i| i as f64 / n as f64).collect();

  // sort indexes by frequency, lower -> higher
  let mut indexes: Vec<usize> = (0..n).collect();
  indexes.sort_by(|&a, &b| f[a].abs().partial_cmp(&f[b].abs()).unwrap());

  let t: Array1<f64> = Array1::range(0.0, (n + n_predict) as f64, 1.0);
  let mut restored_sig = Array1::<f64>::zeros(t.len());
  for &i in indexes.iter().take(1 + n_harm * 2) {
    let ampli = x_freqdom[i].norm() / (n as f64 / 2.0); // amplitude
    let phase = x_freqdom[i].arg(); // phase
    restored_sig += &(ampli * (2.0 * PI * f[i] * &t + phase).mapv(|x| x.cos()));
  }
  restored_sig + slope * t + intercept
}
