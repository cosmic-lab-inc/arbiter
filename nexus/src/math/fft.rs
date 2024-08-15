use crate::{Data, Dataset};
use ndarray::Array1;
use rustfft::num_complex::Complex;
use rustfft::num_traits::Zero;
use rustfft::FftPlanner;
use std::cmp::Ordering;

pub struct Freq {
  pub mag: f64,
  pub period: f64,
}

pub struct FFT {
  pub original: Dataset,
  pub filtered: Dataset,
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
  let filtered_input: Vec<f64> = top_ifft_input.iter().map(|x| x.re).collect();
  let filtered_data: Vec<Data> = filtered_input
    .iter()
    .enumerate()
    .map(|(i, &y)| Data { x: i as i64, y })
    .collect();

  Ok(FFT {
    original: Dataset::new(original_data),
    filtered: Dataset::new(filtered_data),
  })
}
