use crate::Dataset;

pub struct Freq {
  pub mag: f64,
  pub period: f64,
}

pub struct FFT {
  pub original: Dataset,
  pub filtered: Dataset,
}
