use crate::Dataset;

#[derive(Debug, Copy, Clone)]
pub enum EntropyBits {
  One,
  Two,
  Three,
  Four,
}
impl EntropyBits {
  pub fn bits(&self) -> usize {
    match self {
      EntropyBits::One => 1,
      EntropyBits::Two => 2,
      EntropyBits::Three => 3,
      EntropyBits::Four => 4,
    }
  }

  pub fn patterns(&self) -> usize {
    match self {
      EntropyBits::One => 2,
      EntropyBits::Two => 3,
      EntropyBits::Three => 4,
      EntropyBits::Four => 5,
    }
  }
}

#[derive(Debug, Clone, Copy)]
pub enum EntropySignal {
  Up,
  Down,
  None,
}
impl PartialEq for EntropySignal {
  fn eq(&self, other: &Self) -> bool {
    matches!(
      (self, other),
      (EntropySignal::Up, EntropySignal::Up)
        | (EntropySignal::Down, EntropySignal::Down)
        | (EntropySignal::None, EntropySignal::None)
    )
  }
}
impl Eq for EntropySignal {}
impl EntropySignal {
  pub fn signal(&self) -> i8 {
    match self {
      EntropySignal::Up => 1,
      EntropySignal::Down => -1,
      EntropySignal::None => 0,
    }
  }
}

/// Based on this blog: https://robotwealth.com/shannon-entropy/
///
/// Translated from Zorro's `ShannonEntropy` indicator, written in C: https://financial-hacker.com/is-scalping-irrational/
///
/// patterns = number of bits + 1.
///
/// If patterns is 3, and the entropy is 3, then the time series is perfectly random.
/// Anything less than the pattern_size means there exists some regularity and therefore predictability.
pub fn shannon_entropy(data: &[f64], length: usize, patterns: usize) -> f64 {
  let mut s = [0u8; 1024]; // hack!
  let size = std::cmp::min(length - patterns - 1, 1024);
  for i in 0..size {
    let mut c = 0;
    for j in 0..patterns {
      if data[i + j] > data[i + j + 1] {
        c += 1 << j;
      }
    }
    s[i] = c as u8;
  }

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

// ==========================================================================================
//                                     Entropy Tests
// ==========================================================================================

#[cfg(test)]
mod tests {
  use crate::*;

  #[test]
  fn entropy_statistics() -> anyhow::Result<()> {
    use tradestats::metrics::{
      pearson_correlation_coefficient, rolling_correlation, rolling_zscore,
    };

    let start_time = Time::new(2017, 1, 1, None, None, None);
    let end_time = Time::new(2025, 1, 1, None, None, None);
    let timeframe = "1d";

    let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
    let ticker = "BTC".to_string();
    let btc_series =
      Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;
    // let btc_series = Dataset::new(btc_series.0.into_iter().take(1000).collect());

    let period = 100;
    let bits = EntropyBits::Two;

    let mut cache = RingBuffer::new(period + bits.bits(), ticker);

    struct Entry {
      pub price: f64,
      pub delta: f64,
      pub price_zscore: f64,
      pub entropy: f64,
      #[allow(dead_code)]
      pub entropy_signal: EntropySignal,
    }

    let mut dataset = vec![];
    for data in btc_series.data().clone().into_iter() {
      cache.push(data);
      if cache.vec.len() < period + bits.bits() {
        continue;
      }
      let series = Dataset::new(cache.vec());

      let (in_sample, out_sample) = series.sample(bits.bits());
      assert_eq!(in_sample.len(), period);
      assert_eq!(out_sample.len(), bits.bits());

      let closes = in_sample.y();
      let future = out_sample.0[out_sample.0.len() - 1].y;
      let present = closes[closes.len() - 1];
      let delta = (future - present) / present * 100.0;
      let price_zscore = zscore(closes.as_slice(), period).unwrap();

      let entropy = shannon_entropy(closes.as_slice(), period, bits.patterns());
      let entropy_signal = n_bit_entropy!(bits.bits(), period, closes)?;
      dataset.push(Entry {
        price: present,
        delta,
        price_zscore,
        entropy,
        entropy_signal,
      });
    }

    // iterate and calculate correlation between entropy and delta, and entropy and price zscore
    let p = dataset.iter().map(|entry| entry.price).collect();
    let pz = dataset.iter().map(|entry| entry.price_zscore).collect();
    let d = dataset.iter().map(|entry| entry.delta).collect();
    let dz = rolling_zscore(&d, period).unwrap();
    let e = dataset.iter().map(|entry| entry.entropy).collect();
    let ez = rolling_zscore(&e, period).unwrap();

    let pz_ez_corr = pearson_correlation_coefficient(&pz, &ez).unwrap();
    let roll_pz_ez_corr = rolling_correlation(&pz, &ez, period).unwrap();
    println!("pz : ez,  corr: {}", pz_ez_corr);

    let pz_e_corr = pearson_correlation_coefficient(&pz, &e).unwrap();
    let roll_pz_e_corr = rolling_correlation(&pz, &e, period).unwrap();
    println!("pz : e,  corr: {}", pz_e_corr);

    let p_e_corr = pearson_correlation_coefficient(&p, &e).unwrap();
    let roll_p_e_corr = rolling_correlation(&p, &e, period).unwrap();
    println!("p : e,  corr: {}", p_e_corr);

    let d_ez_corr = pearson_correlation_coefficient(&d, &ez).unwrap();
    let roll_d_ez_corr = rolling_correlation(&dz, &ez, period).unwrap();
    println!("d : ez,  corr: {}", d_ez_corr);

    let pz_dataset = Dataset::from(pz);
    let ez_dataset = Dataset::from(ez);
    let roll_pz_ez_dataset = Dataset::from(roll_pz_ez_corr);
    let roll_pz_e_dataset = Dataset::from(roll_pz_e_corr);
    let roll_p_e_dataset = Dataset::from(roll_p_e_corr);
    let roll_d_ez_dataset = Dataset::from(roll_d_ez_corr);

    Plot::plot(
      vec![
        Series {
          data: roll_pz_ez_dataset.0,
          label: "PZ : EZ Corr".to_string(),
        },
        Series {
          data: roll_pz_e_dataset.0,
          label: "PZ : E Corr".to_string(),
        },
        Series {
          data: roll_p_e_dataset.0,
          label: "P : E Corr".to_string(),
        },
        Series {
          data: roll_d_ez_dataset.0,
          label: "D : EZ Corr".to_string(),
        },
      ],
      "corr.png",
      "Price v Entropy Correlation",
      "Correlation",
      "Time",
      Some(false),
    )?;

    Plot::plot(
      vec![
        Series {
          data: pz_dataset.0,
          label: "Price Z-Score".to_string(),
        },
        Series {
          data: ez_dataset.0,
          label: "Entropy Z-Score".to_string(),
        },
      ],
      "zscore.png",
      "Price Z-Score v Entropy Z-Score",
      "Z Score",
      "Time",
      Some(false),
    )?;

    Ok(())
  }

  #[test]
  fn entropy_ema() -> anyhow::Result<()> {
    let start_time = Time::new(2022, 1, 1, None, None, None);
    let end_time = Time::new(2022, 2, 1, None, None, None);
    let timeframe = "1d";

    let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
    let ticker = "BTC".to_string();
    let btc_series =
      Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

    let period = 5;
    let bits = 4;
    let patterns = bits + 1;

    let mut cache = RingBuffer::new(period + bits, ticker);

    struct Entry {
      pub price: f64,
      pub ema: f64,
      pub ft: f64,
      pub ft_e: f64,
    }

    let mut dataset = vec![];
    for data in btc_series.data().clone().into_iter() {
      cache.push(data);
      if cache.vec.len() < period + bits {
        continue;
      }
      let series = Dataset::new(cache.vec());

      let (in_sample, out_sample) = series.sample(bits);
      assert_eq!(in_sample.len(), period);
      assert_eq!(out_sample.len(), bits);

      let closes = in_sample.y();
      let ema = ema(&closes);

      let mut cum_pos = 0.0;
      let mut pos = vec![];
      let mut cum_neg = 0.0;
      let mut neg = vec![];
      for close in closes.iter().cloned() {
        if close > ema {
          cum_pos += close;
          pos.push(close);
        } else {
          cum_neg += close;
          neg.push(close);
        }
      }
      let pos_e = shannon_entropy(pos.as_slice(), period + 1, patterns);
      let neg_e = shannon_entropy(neg.as_slice(), period + 1, patterns);
      let ft = (cum_neg - cum_pos) / (cum_neg + cum_pos);
      let ft_e = (neg_e - pos_e) / (neg_e + pos_e);
      dataset.push(Entry {
        price: closes[closes.len() - 1],
        ema,
        ft,
        ft_e,
      });
    }

    let p = Dataset::from(
      dataset
        .iter()
        .map(|entry| entry.price)
        .collect::<Vec<f64>>(),
    );
    let ema = Dataset::from(dataset.iter().map(|entry| entry.ema).collect::<Vec<f64>>());
    let ft = Dataset::from(dataset.iter().map(|entry| entry.ft).collect::<Vec<f64>>());
    let ft_e = Dataset::from(dataset.iter().map(|entry| entry.ft_e).collect::<Vec<f64>>());

    Plot::plot_dual_axis(DualAxisConfig {
      out_file: "ft.png",
      title: "Entropy Fast Trend",
      x_label: "Time",
      series: [
        Series {
          data: p.0,
          label: "Price".to_string(),
        },
        Series {
          data: ema.0,
          label: "EMA".to_string(),
        },
      ],
      y_label: "Price",
      log_scale: Some(false),
      second_axis_series: [
        Series {
          data: ft.0,
          label: "Price FT".to_string(),
        },
        Series {
          data: ft_e.0,
          label: "Entropy FT".to_string(),
        },
      ],
      second_axis_y_label: "Entropy",
      second_axis_log_scale: Some(false),
    })?;

    Ok(())
  }

  /// Uses future bars to confirm whether entropy prediction was correct.
  /// This is not to be used directly in a backtest, since future data is impossible.
  #[test]
  fn entropy_n_step() -> anyhow::Result<()> {
    use super::*;
    dotenv::dotenv().ok();
    init_logger();

    let clock_start = Time::now();
    let start_time = Time::new(2019, 1, 1, None, None, None);
    let end_time = Time::new(2021, 1, 1, None, None, None);
    let timeframe = "1h";

    let btc_csv = workspace_path(&format!("data/btc_{}.csv", timeframe));
    let ticker = "BTC".to_string();
    let btc_series =
      Dataset::csv_series(&btc_csv, Some(start_time), Some(end_time), ticker.clone())?;

    let bits = 2;
    let patterns = bits + 1;
    let period = 100;

    let mut longs = 0;
    let mut shorts = 0;
    let mut long_win = 0;
    let mut short_win = 0;
    let mut cum_pnl = 0.0;

    let mut entropies = vec![];
    let mut pnl_series = vec![];
    for i in 0..btc_series.y().len() - period + 1 - patterns {
      let series = btc_series.y()[i..i + period + patterns].to_vec();

      let trained = btc_series.y()[i..i + period].to_vec();
      assert_eq!(trained.len(), period);
      let expected = btc_series.y()[i + period..i + period + patterns].to_vec();
      assert_eq!(expected.len(), patterns);

      let eli = expected.len() - 1;
      let p0 = expected[eli];
      let pn = expected[eli - bits];

      let signal = n_bit_entropy!(bits, period, trained)?;

      let up = matches!(signal, EntropySignal::Up);
      let down = matches!(signal, EntropySignal::Down);

      if up {
        longs += 1;
        if pn < p0 {
          // long && new price > old price, long wins
          long_win += 1;
          cum_pnl += p0 - pn;
        } else {
          // long && new price < old price, long loses
          cum_pnl += p0 - pn;
        }
      } else if down {
        shorts += 1;
        if pn > p0 {
          // short && old price > new price, short wins
          short_win += 1;
          cum_pnl += pn - p0;
        } else {
          // short && old price < new price, short loses
          cum_pnl += pn - p0;
        }
      }

      pnl_series.push(cum_pnl);
      entropies.push(shannon_entropy(series.as_slice(), period, patterns));
    }
    let avg_entropy = entropies.iter().sum::<f64>() / entropies.len() as f64;
    println!("entropy: {}/{}", trunc!(avg_entropy, 3), patterns);

    let trades = longs + shorts;
    let win_rate = trunc!((long_win + short_win) as f64 / trades as f64 * 100.0, 2);
    let long_win_rate = trunc!(long_win as f64 / longs as f64 * 100.0, 2);
    let short_win_rate = trunc!(short_win as f64 / shorts as f64 * 100.0, 2);
    println!(
      "trades: {}, long: {}/{}, win rate: {}%, long WR: {}%, short WR: {}%, pnl: ${}",
      trades,
      longs,
      longs + shorts,
      win_rate,
      long_win_rate,
      short_win_rate,
      trunc!(cum_pnl, 2)
    );

    println!(
      "finished test in: {}s",
      Time::now().to_unix() - clock_start.to_unix()
    );

    Ok(())
  }
}
