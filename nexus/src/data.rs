use crate::{Bar, Time};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::File;
use std::path::PathBuf;
use std::str::FromStr;

pub trait Y: Clone {
  fn y(&self) -> f64;
}

pub trait X: Clone {
  fn x(&self) -> i64;
}

impl Y for f64 {
  fn y(&self) -> f64 {
    *self
  }
}

impl X for i64 {
  fn x(&self) -> i64 {
    *self
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Data {
  pub x: i64,
  pub y: f64,
}

impl Y for Data {
  fn y(&self) -> f64 {
    self.y.y()
  }
}

impl X for Data {
  fn x(&self) -> i64 {
    self.x.x()
  }
}

pub struct CsvSeries {
  pub bars: Vec<Bar>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dataset(pub Vec<Data>);

impl Dataset {
  pub fn new(data: Vec<Data>) -> Self {
    Self(data)
  }

  pub fn asc_order(&self) -> Vec<Data> {
    // sort so data.x is in ascending order (highest value is 0th index)
    let mut data = self.0.clone();
    data.sort_by_key(|a| a.x());
    data
  }

  pub fn x(&self) -> Vec<i64> {
    self.0.iter().map(|d| d.x()).collect()
  }

  pub fn y(&self) -> Vec<f64> {
    self.0.iter().map(|d| d.y()).collect()
  }

  pub fn data(&self) -> &Vec<Data> {
    &self.0
  }

  pub fn len(&self) -> usize {
    self.0.len()
  }

  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }

  /// Read candles from CSV file.
  /// Handles duplicate candles and sorts candles by date.
  /// Expects date of candle to be in UNIX timestamp format.
  /// CSV format: date,open,high,low,close,volume
  pub fn csv_series(
    csv_path: &PathBuf,
    start_time: Option<Time>,
    end_time: Option<Time>,
    _ticker: String,
  ) -> anyhow::Result<Dataset> {
    let file_buffer = File::open(csv_path)?;
    let mut csv = csv::Reader::from_reader(file_buffer);

    let mut headers = vec![];
    if let Ok(result) = csv.headers() {
      for header in result {
        headers.push(String::from(header));
      }
    }

    let mut bars = vec![];

    for record in csv.records().flatten() {
      let is_unix_ts = record[0].parse::<i64>();
      let is_date = record[0].parse::<String>();
      let date = if let Ok(unix_ts) = is_unix_ts {
        Ok(Time::from_unix(unix_ts))
      } else if let Ok(date) = is_date {
        // format is: 2020-08-11 06:00:00
        let dt = NaiveDateTime::parse_from_str(&date, "%Y-%m-%d %H:%M:%S")?;
        Ok(Time::from_naive_date(dt))
      } else {
        Err(anyhow::anyhow!("Invalid date format: {:?}", &record[0]))
      }?;
      let volume = None;
      bars.push(Bar {
        date,
        open: f64::from_str(&record[1])?,
        high: f64::from_str(&record[2])?,
        low: f64::from_str(&record[3])?,
        close: f64::from_str(&record[4])?,
        volume,
      });
    }
    // only take candles greater than a timestamp
    bars.retain(|candle| match (start_time, end_time) {
      (Some(start), Some(end)) => {
        candle.date.to_unix_ms() > start.to_unix_ms() && candle.date.to_unix_ms() < end.to_unix_ms()
      }
      (Some(start), None) => candle.date.to_unix_ms() > start.to_unix_ms(),
      (None, Some(end)) => candle.date.to_unix_ms() < end.to_unix_ms(),
      (None, None) => true,
    });

    let data = bars
      .iter()
      .map(|candle| Data {
        x: candle.date.to_unix_ms(),
        y: candle.close,
      })
      .collect();
    Ok(Dataset::new(data))
  }

  /// Redefine each price point as a percentage change relative to the starting price.
  pub fn normalize_series(&self) -> anyhow::Result<Dataset> {
    let mut series = self.0.to_vec();
    series.sort_by_key(|c| c.x());
    let d_0 = series.first().unwrap().clone();
    let x: Dataset = Dataset::new(
      series
        .iter()
        .map(|d| Data {
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
        .map(|x| Data {
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
    let first_x: HashSet<i64> = first.x().into_iter().collect();
    let second_x: HashSet<i64> = second.x().into_iter().collect();
    // Step 2: Find the intersection of both timestamp sets
    let common_timestamps: HashSet<&i64> = first_x.intersection(&second_x).collect();
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
