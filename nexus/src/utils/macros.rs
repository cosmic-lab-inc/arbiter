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

    let li = $vec.len() - 1;
    let lv = $vec[li];

    let mut entropies = vec![0_f64; permutations];
    for i in 0..permutations {
      let mut series = $vec.clone();
      // if b1101 then j iterates backwards as 1,0,1,1
      for j in 0..$n {
        let rev_j = $n - j - 1;
        let rev_bit = (i >> rev_j) & 1;
        // --- 2 bit example with pattern b00 ---
        // b00[li - 1] = trained[li] - 1.0;
        // b00[li] = b00[li - 1] - 1.0;
        //
        // --- 3 bit example with pattern b010 ---
        // series[li - 2] = lv - 1.0;
        // series[li - 1] = series[li - 2] + 1.0;
        // series[li] = series[li - 1] - 1.0;
        let prev_value = match j == 0 {
          true => lv,
          false => series[li - (rev_j + 1)],
        };
        if rev_bit == 0 {
          series[li - rev_j] = prev_value - 1.0;
        } else if rev_bit == 1 {
          series[li - rev_j] = prev_value + 1.0;
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
  }};
}
