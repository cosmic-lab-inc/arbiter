#[macro_export]
macro_rules! trunc {
  ($num:expr, $decimals:expr) => {{
    let factor = 10.0_f64.powi($decimals);
    ($num * factor).round() / factor
  }};
}

#[macro_export]
macro_rules! n_bit_entropy {
  ($n:expr, $period:expr, $vec:expr) => {{
    let permutations = 2usize.pow($n as u32);

    // let avg_bar_delta = $vec.windows(2).map(|w| w[1] - w[0]).sum::<f64>() / ($vec.len() - 1) as f64;
    // let delta = avg_bar_delta.sqrt();
    let delta = 1.0;

    let li = $vec.len() - 1;
    let lv = $vec[li];

    let mut entropies = vec![0_f64; permutations];
    for i in 0..permutations {
      let mut series = $vec.clone();
      // if b1101 then j iterates backwards as 1,0,1,1
      for j in 0..$n {
        let rev_j = $n - j - 1;
        let rev_bit = (i >> rev_j) & 1;
        let prev_value = match j == 0 {
          true => lv,
          false => series[li - (rev_j + 1)],
        };
        if rev_bit == 0 {
          series[li - rev_j] = prev_value - delta;
        } else if rev_bit == 1 {
          series[li - rev_j] = prev_value + delta;
        }
      }
      let entropy = $crate::shannon_entropy(&series, $period + 1, $n + 1);
      entropies[i] = entropy;
    }

    let low = entropies[0].clone();
    let high = entropies[permutations - 1].clone();
    let mut max = high;
    for e in entropies {
      max = max.max(e);
    }
    let up = max == low && low != high;
    let down = max == high && low != high;
    Result::<_, anyhow::Error>::Ok(if up {
      $crate::EntropySignal::Up
    } else if down {
      $crate::EntropySignal::Down
    } else {
      $crate::EntropySignal::None
    })

    // let low = entropies[0].clone();
    // let high = entropies[permutations - 1].clone();
    // let mut min = high;
    // for e in entropies {
    //   min = min.min(e);
    // }
    // let up = min == high && low != high;
    // let down = min == low && low != high;
    // Result::<_, anyhow::Error>::Ok(if up {
    //   $crate::EntropySignal::Up
    // } else if down {
    //   $crate::EntropySignal::Down
    // } else {
    //   $crate::EntropySignal::None
    // })
  }};
}
