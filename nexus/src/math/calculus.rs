// take the derivative of the series
pub fn derivative(series: &[f64]) -> Vec<f64> {
  let mut result = Vec::new();
  for i in 0..series.len() - 1 {
    result.push(series[i + 1] - series[i]);
  }
  result
}

pub fn slope(data: &[f64]) -> f64 {
  let mut sum = 0.0;
  for i in 1..data.len() {
    sum += data[i] - data[i - 1];
  }
  sum / data.len() as f64
}
