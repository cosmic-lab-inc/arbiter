use crate::{Time, X, Y};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Event for a single bar for a given ticker.
#[derive(Clone, Copy, Debug)]
pub struct Bar {
  /// UNIX timestamp in seconds
  pub date: Time,
  /// Open price
  pub open: f64,
  /// High price
  pub high: f64,
  /// Low price
  pub low: f64,
  /// Close price
  pub close: f64,
  /// Volume
  pub volume: Option<f64>,
}

impl Y for Bar {
  fn y(&self) -> f64 {
    self.close
  }
}

impl X for Bar {
  fn x(&self) -> i64 {
    self.date.to_unix_ms()
  }
}

impl Bar {
  pub fn percent_change(&self, prev_close: f64) -> f64 {
    ((100.0 / prev_close) * self.close) - 100.0
  }
}

impl PartialEq for Bar {
  fn eq(&self, other: &Self) -> bool {
    self.date.to_string() == other.date.to_string() && self.close == other.close
  }
}

pub trait BarTrait {
  fn unix_date(&self) -> u64;
}

impl BarTrait for Bar {
  fn unix_date(&self) -> u64 {
    self.date.to_unix() as u64
  }
}

#[derive(Clone, Debug, Default)]
pub struct BarHasher(pub DefaultHasher);

pub trait BarHashTrait {
  fn new() -> Self;
  fn finish(&mut self) -> u64;
  fn hash_bar<T: BarTrait>(&mut self, bar: &T) -> u64;
}

impl BarHashTrait for BarHasher {
  fn new() -> Self {
    Self(DefaultHasher::new())
  }
  /// Reset contents of hasher for reuse
  fn finish(&mut self) -> u64 {
    self.0.finish()
  }
  /// Hash account using key and slot
  fn hash_bar<T: BarTrait>(&mut self, bar: &T) -> u64 {
    self.0 = DefaultHasher::new();
    bar.unix_date().hash(&mut self.0);
    self.finish()
  }
}
