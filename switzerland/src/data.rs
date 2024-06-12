use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub trait Y: Clone {
  fn y(&self) -> f64;
}

pub trait X: Clone {
  fn x(&self) -> u64;
}

impl Y for f64 {
  fn y(&self) -> f64 {
    *self
  }
}

impl X for u64 {
  fn x(&self) -> u64 {
    *self
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XY {
  pub x: u64,
  pub y: f64,
}

impl Y for XY {
  fn y(&self) -> f64 {
    self.y.y()
  }
}

impl X for XY {
  fn x(&self) -> u64 {
    self.x.x()
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dataset(pub Vec<XY>);

impl Dataset {
  pub fn new(data: Vec<XY>) -> Self {
    Self(data)
  }

  pub fn asc_order(&self) -> Vec<XY> {
    // sort so data.x is in ascending order (highest value is 0th index)
    let mut data = self.0.clone();
    data.sort_by_key(|a| a.x());
    data
  }

  pub fn x(&self) -> Vec<u64> {
    self.0.iter().map(|d| d.x()).collect()
  }

  pub fn y(&self) -> Vec<f64> {
    self.0.iter().map(|d| d.y()).collect()
  }

  pub fn data(&self) -> &Vec<XY> {
    &self.0
  }

  pub fn len(&self) -> usize {
    self.0.len()
  }

  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }

  /// Redefine each price point as a percentage change relative to the starting price.
  pub fn normalize_series(&self) -> anyhow::Result<Dataset> {
    let mut series = self.0.to_vec();
    series.sort_by_key(|c| c.x());
    let d_0 = series.first().unwrap().clone();
    let x: Dataset = Dataset::new(
      series
        .iter()
        .map(|d| XY {
          x: d.x(),
          y: (d.y() / d_0.y() - 1.0) * 100.0,
        })
        .collect(),
    );
    Ok(x)
  }

  pub fn lagged_spread_series(&self) -> anyhow::Result<Dataset> {
    let mut series = self.0.to_vec();
    series.sort_by_key(|c| c.x());
    let spread: Dataset = Dataset::new(
      series
        .windows(2)
        .map(|x| XY {
          x: x[1].x(),
          y: x[1].y() - x[0].y(),
        })
        .collect(),
    );
    Ok(spread)
  }

  pub fn align(first: &mut Dataset, second: &mut Dataset) -> anyhow::Result<()> {
    // retain the overlapping dates between the two time series
    // Step 1: Create sets of timestamps from both vectors
    let first_x: HashSet<u64> = first.x().into_iter().collect();
    let second_x: HashSet<u64> = second.x().into_iter().collect();
    // Step 2: Find the intersection of both timestamp sets
    let common_timestamps: HashSet<&u64> = first_x.intersection(&second_x).collect();
    // Step 3: Filter each vector to keep only the common timestamps
    first.0.retain(|c| common_timestamps.contains(&c.x));
    second.0.retain(|c| common_timestamps.contains(&c.x));
    // Step 4: Sort both vectors by timestamp to ensure they are aligned
    // earliest point in time is 0th index, latest point in time is Nth index
    first.0.sort_by_key(|c| c.x);
    second.0.sort_by_key(|c| c.x);
    Ok(())
  }
}
