#![allow(dead_code)]
#![allow(clippy::let_and_return)]
#![allow(unused_assignments)]

use crate::drift_client::types::*;
use crate::drift_cpi::*;
use crate::Time;
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive};
use std::ops::{Add, Div, Mul, Sub};

pub struct AmmUtils;

impl AmmUtils {
  pub fn bid_ask_prices(market: PerpMarket, oracle: OraclePriceData, with_update: bool) -> BidAsk {
    let amm = if with_update {
      Self::calc_updated_amm(&market.amm, oracle)
    } else {
      market.amm
    };

    let SpreadReserves {
      bid_reserves,
      ask_reserves,
    } = Self::calc_spread_reserves(&amm, oracle);
    let bid = Self::calc_price(
      bid_reserves.base_asset_reserve,
      bid_reserves.quote_asset_reserve,
      BigInt::from(amm.peg_multiplier),
    );
    let ask = Self::calc_price(
      ask_reserves.base_asset_reserve,
      ask_reserves.quote_asset_reserve,
      BigInt::from(amm.peg_multiplier),
    );
    BidAsk {
      bid: bid.clone().to_f64().unwrap() / PRICE_PRECISION as f64,
      ask: ask.clone().to_f64().unwrap() / PRICE_PRECISION as f64,
    }
  }

  pub fn ask_price(market: PerpMarket, oracle: OraclePriceData) -> f64 {
    let UpdatedAmmSpreadReserves {
      base_asset_reserve,
      quote_asset_reserve,
      new_peg,
      ..
    } = Self::calc_updated_amm_spread_reserves(&market.amm, PositionDirection::Long, oracle);
    let price = Self::calc_price(base_asset_reserve, quote_asset_reserve, new_peg);
    price.to_f64().unwrap() / PRICE_PRECISION as f64
  }

  pub fn bid_price(market: PerpMarket, oracle: OraclePriceData) -> f64 {
    let UpdatedAmmSpreadReserves {
      base_asset_reserve,
      quote_asset_reserve,
      new_peg,
      ..
    } = Self::calc_updated_amm_spread_reserves(&market.amm, PositionDirection::Short, oracle);
    let price = Self::calc_price(base_asset_reserve, quote_asset_reserve, new_peg);
    price.to_f64().unwrap() / PRICE_PRECISION as f64
  }

  fn calc_updated_amm(amm: &AMM, oracle: OraclePriceData) -> AMM {
    let mut new_amm = *amm;
    if amm.curve_update_intensity == 0 {
      return new_amm;
    }
    let NewAmm {
      pre_peg_cost,
      pk_numer,
      pk_denom,
      new_peg,
    } = Self::calc_new_amm(amm, oracle);

    new_amm.base_asset_reserve = BigInt::from(new_amm.base_asset_reserve)
      .mul(pk_numer.clone())
      .div(pk_denom.clone())
      .to_u128()
      .unwrap();
    new_amm.sqrt_k = BigInt::from(new_amm.sqrt_k)
      .mul(pk_numer)
      .div(pk_denom)
      .to_u128()
      .unwrap();
    let invariant = new_amm.sqrt_k * new_amm.sqrt_k;
    new_amm.quote_asset_reserve = invariant.div(new_amm.base_asset_reserve);
    new_amm.peg_multiplier = new_peg.to_u128().unwrap();

    let close_dir = if amm.base_asset_amount_with_amm.gt(&0) {
      PositionDirection::Short
    } else {
      PositionDirection::Long
    };
    let AmmReservesAfterSwap {
      new_quote_asset_reserve,
      ..
    } = Self::calc_amm_reserves_after_swap(
      &new_amm,
      AssetType::Base,
      BigInt::from(amm.base_asset_amount_with_amm.abs()),
      Self::swap_direction(AssetType::Base, close_dir),
    );
    new_amm.terminal_quote_asset_reserve = new_quote_asset_reserve.to_u128().unwrap();
    new_amm.total_fee_minus_distributions = BigInt::from(new_amm.total_fee_minus_distributions)
      .sub(pre_peg_cost.clone())
      .to_i128()
      .unwrap();
    new_amm.net_revenue_since_last_funding = BigInt::from(new_amm.net_revenue_since_last_funding)
      .sub(pre_peg_cost)
      .to_i64()
      .unwrap();
    new_amm
  }

  fn calc_new_amm(amm: &AMM, oracle: OraclePriceData) -> NewAmm {
    let mut pk_numer = BigInt::from(1);
    let mut pk_denom = BigInt::from(1);
    let OptimalPegAndBudget {
      target_price,
      mut new_peg,
      budget,
      ..
    } = Self::calc_optimal_peg_and_budget(amm, oracle);
    let mut pre_peg_cost = Self::calc_repeg_cost(amm, new_peg.clone());

    if pre_peg_cost.ge(&budget) && pre_peg_cost.gt(&BigInt::from(0)) {
      pk_numer = BigInt::from(999);
      pk_denom = BigInt::from(1000);
      let deficit_made_up = Self::calc_adjust_k_cost(amm, pk_numer.clone(), pk_denom.clone());
      assert!(deficit_made_up.le(&BigInt::from(0)));
      pre_peg_cost = budget.add(BigInt::from(deficit_made_up.abs()));
      let mut new_amm = amm.clone();
      new_amm.base_asset_reserve = BigInt::from(new_amm.base_asset_reserve)
        .mul(pk_numer.clone())
        .div(pk_denom.clone())
        .to_u128()
        .unwrap();
      new_amm.sqrt_k = BigInt::from(new_amm.sqrt_k)
        .mul(pk_numer.clone())
        .div(pk_denom.clone())
        .to_u128()
        .unwrap();
      let invariant = new_amm.sqrt_k.mul(new_amm.sqrt_k);
      new_amm.quote_asset_reserve = invariant.div(new_amm.base_asset_reserve);
      let close_dir = if amm.base_asset_amount_with_amm.gt(&0) {
        PositionDirection::Short
      } else {
        PositionDirection::Long
      };
      let AmmReservesAfterSwap {
        new_quote_asset_reserve,
        ..
      } = Self::calc_amm_reserves_after_swap(
        &new_amm,
        AssetType::Base,
        BigInt::from(amm.base_asset_amount_with_amm.abs()),
        Self::swap_direction(AssetType::Base, close_dir),
      );
      new_amm.terminal_quote_asset_reserve = new_quote_asset_reserve.to_u128().unwrap();
      new_peg = Self::calc_budgeted_peg(&new_amm, pre_peg_cost, target_price);
      pre_peg_cost = Self::calc_repeg_cost(&new_amm, new_peg.clone());
    }

    NewAmm {
      pre_peg_cost,
      pk_numer,
      pk_denom,
      new_peg,
    }
  }

  fn calc_updated_amm_spread_reserves(
    amm: &AMM,
    dir: PositionDirection,
    oracle: OraclePriceData,
  ) -> UpdatedAmmSpreadReserves {
    let new_amm = Self::calc_updated_amm(amm, oracle);
    let SpreadReserves {
      bid_reserves: long_reserves,
      ask_reserves: short_reserves,
    } = Self::calc_spread_reserves(&new_amm, oracle);
    let dir_reserves = if matches!(dir, PositionDirection::Long) {
      long_reserves
    } else {
      short_reserves
    };
    UpdatedAmmSpreadReserves {
      base_asset_reserve: dir_reserves.base_asset_reserve,
      quote_asset_reserve: dir_reserves.quote_asset_reserve,
      sqrt_k: BigInt::from(new_amm.sqrt_k),
      new_peg: BigInt::from(new_amm.peg_multiplier),
    }
  }

  fn calc_spread_reserves(amm: &AMM, oracle: OraclePriceData) -> SpreadReserves {
    let calc_spread_reserve =
      |spread: BigInt, _dir: PositionDirection, amm: &AMM| -> SpreadReserve {
        if spread.eq(&BigInt::from(0)) {
          return SpreadReserve {
            base_asset_reserve: BigInt::from(amm.base_asset_reserve),
            quote_asset_reserve: BigInt::from(amm.quote_asset_reserve),
          };
        }
        let mut spread_fraction = spread.clone().div(BigInt::from(2));
        if spread_fraction.eq(&BigInt::from(0)) {
          spread_fraction = if spread.ge(&BigInt::from(0)) {
            BigInt::from(1)
          } else {
            BigInt::from(-1)
          };
        }
        let quote_asset_reserve_delta = BigInt::from(amm.quote_asset_reserve)
          .div(BID_ASK_SPREAD_PRECISION)
          .div(spread_fraction);
        let quote_asset_reserve = if quote_asset_reserve_delta.ge(&BigInt::from(0)) {
          BigInt::from(amm.quote_asset_reserve).add(quote_asset_reserve_delta)
        } else {
          BigInt::from(amm.quote_asset_reserve).sub(quote_asset_reserve_delta)
        };
        let base_asset_reserve = BigInt::from(amm.sqrt_k)
          .mul(BigInt::from(amm.sqrt_k))
          .div(&quote_asset_reserve);
        SpreadReserve {
          base_asset_reserve,
          quote_asset_reserve,
        }
      };

    let reserve_price = Self::calc_price(
      BigInt::from(amm.base_asset_reserve),
      BigInt::from(amm.quote_asset_reserve),
      BigInt::from(amm.peg_multiplier),
    );
    let mut max_offset = BigInt::from(0);
    let mut reference_price_offset = BigInt::from(0);
    if amm.curve_update_intensity > 100 {
      let other =
        (PERCENTAGE_PRECISION.div(10_000)) * (BigInt::from(amm.curve_update_intensity).sub(100));
      max_offset = BigInt::from(amm.max_spread).div(BigInt::from(5)).max(other);

      let liq_frac = Self::calc_inventory_liquidity_ratio(
        BigInt::from(amm.base_asset_amount_with_amm),
        BigInt::from(amm.base_asset_reserve),
        BigInt::from(amm.min_base_asset_reserve),
        BigInt::from(amm.max_base_asset_reserve),
      );

      let liq_frac_signed = BigInt::from(liq_frac).mul(Self::sig_num(
        BigInt::from(amm.base_asset_amount_with_amm)
          .add(BigInt::from(amm.base_asset_amount_with_unsettled_lp)),
      ));

      reference_price_offset =
        Self::calc_reference_point_offset(amm, reserve_price.clone(), liq_frac_signed, max_offset);
    }; // end of inner fn

    let Spread {
      long_spread,
      short_spread,
    } = Self::calc_spread(amm, oracle, None, Some(reserve_price));

    let ask_reserves = calc_spread_reserve(
      long_spread.add(&reference_price_offset),
      PositionDirection::Long,
      amm,
    );
    let bid_reserves = calc_spread_reserve(
      short_spread
        .mul(BigInt::from(-1))
        .add(reference_price_offset),
      PositionDirection::Short,
      amm,
    );

    SpreadReserves {
      bid_reserves,
      ask_reserves,
    }
  }

  fn calc_spread(
    amm: &AMM,
    oracle: OraclePriceData,
    now: Option<i64>,
    reserve_price: Option<BigInt>,
  ) -> Spread {
    let mut reserve_price = reserve_price;
    if amm.base_spread.eq(&0) || amm.curve_update_intensity.eq(&0) {
      return Spread {
        long_spread: BigInt::from(amm.base_spread.div(2)),
        short_spread: BigInt::from(amm.base_spread.div(2)),
      };
    }
    if reserve_price.is_none() {
      reserve_price = Some(Self::calc_price(
        BigInt::from(amm.base_asset_reserve),
        BigInt::from(amm.quote_asset_reserve),
        BigInt::from(amm.peg_multiplier),
      ));
    }
    let reserve_price = reserve_price.unwrap();
    let target_price = oracle.price;
    let target_mark_spread_pct = reserve_price
      .clone()
      .sub(target_price)
      .mul(BID_ASK_SPREAD_PRECISION_I64)
      .div(&reserve_price);
    let now = now.unwrap_or_else(|| Time::now().to_unix());
    let live_oracle_std = Self::calc_live_oracle_std(amm, oracle, now);
    let conf_interval_pct = Self::new_oracle_conf_pct(amm, oracle, reserve_price.clone(), now);
    Self::calc_spread_inner(
      amm,
      target_mark_spread_pct,
      conf_interval_pct,
      reserve_price,
      live_oracle_std,
    )
  }

  fn calc_spread_inner(
    amm: &AMM,
    last_oracle_reserve_price_spread_pct: BigInt,
    last_oracle_conf_pct: BigInt,
    reserve_price: BigInt,
    oracle_std: BigInt,
  ) -> Spread {
    #[derive(Default)]
    struct SpreadTerms {
      long_vol_spread: BigInt,
      short_vol_spread: BigInt,
      long_spread_w_ps: BigInt,
      short_spread_w_ps: BigInt,
      max_target_spread: BigInt,
      inventory_spread_scale: BigInt,
      long_spread_w_inv_scale: BigInt,
      short_spread_w_inv_scale: BigInt,
      effective_leverage: BigInt,
      effective_leverage_capped: BigInt,
      long_spread_w_el: BigInt,
      short_spread_w_el: BigInt,
      revenue_retreat_amount: BigInt,
      half_revenue_retreat_amount: BigInt,
      long_spread_w_rev_retreat: BigInt,
      short_spread_w_rev_retreat: BigInt,
      long_spread_w_offset_shrink: BigInt,
      short_spread_w_offset_shrink: BigInt,
      total_spread: BigInt,
      long_spread: BigInt,
      short_spread: BigInt,
    }
    let mut terms = SpreadTerms::default();

    let VolSpread {
      long_vol_spread,
      short_vol_spread,
    } = Self::calc_vol_spread(
      last_oracle_conf_pct.clone(),
      reserve_price.clone(),
      BigInt::from(amm.mark_std),
      oracle_std.clone(),
      BigInt::from(amm.long_intensity_volume),
      BigInt::from(amm.short_intensity_volume),
      BigInt::from(amm.volume24h),
    );
    terms.long_vol_spread = long_vol_spread.clone();
    terms.short_vol_spread = short_vol_spread.clone();

    let mut long_spread = BigInt::from(amm.base_spread)
      .div(BigInt::from(2))
      .max(long_vol_spread.clone());
    let mut short_spread = BigInt::from(amm.base_spread)
      .div(BigInt::from(2))
      .max(short_vol_spread.clone());

    if last_oracle_reserve_price_spread_pct.gt(&BigInt::from(0)) {
      short_spread = short_spread.clone().max(
        last_oracle_reserve_price_spread_pct
          .clone()
          .abs()
          .add(short_vol_spread),
      );
    } else if last_oracle_reserve_price_spread_pct.lt(&BigInt::from(0)) {
      long_spread = long_spread.clone().max(
        last_oracle_reserve_price_spread_pct
          .clone()
          .abs()
          .add(long_vol_spread),
      );
    }
    terms.long_spread_w_ps = long_spread.clone();
    terms.short_spread_w_ps = short_spread.clone();

    let max_spread_baseline = {
      let a = last_oracle_reserve_price_spread_pct.abs();
      let b = last_oracle_conf_pct.clone().mul(BigInt::from(2));
      let c = BigInt::from(amm.mark_std)
        .max(oracle_std.clone())
        .mul(PERCENTAGE_PRECISION)
        .div(reserve_price.clone());
      let first = a.max(b.max(c));
      let second = BigInt::from(BID_ASK_SPREAD_PRECISION);
      first.min(second)
    };

    let max_target_spread = BigInt::from(amm.max_spread).max(max_spread_baseline);

    let inventory_spread_scale = Self::calc_inventory_scale(
      BigInt::from(amm.base_asset_amount_with_amm),
      BigInt::from(amm.base_asset_reserve),
      BigInt::from(amm.min_base_asset_reserve),
      BigInt::from(amm.max_base_asset_reserve),
      match amm.base_asset_amount_with_amm.gt(&0) {
        true => long_spread.clone(),
        false => short_spread.clone(),
      },
      max_target_spread.clone(),
    );

    if amm.base_asset_amount_with_amm.gt(&0) {
      long_spread = long_spread.clone().mul(&inventory_spread_scale);
    } else if amm.base_asset_amount_with_amm.lt(&0) {
      short_spread = short_spread.clone().mul(&inventory_spread_scale);
    }
    terms.max_target_spread = max_target_spread.clone();
    terms.inventory_spread_scale = inventory_spread_scale.clone();
    terms.long_spread_w_inv_scale = long_spread.clone();
    terms.short_spread_w_inv_scale = short_spread.clone();

    let max_spread_scale = BigInt::from(10);
    if amm.total_fee_minus_distributions.gt(&0) {
      let effective_leverage = Self::calc_effective_leverage(amm, reserve_price.clone());
      terms.effective_leverage = effective_leverage.clone();

      let spread_scale = max_spread_scale.min(1 + effective_leverage);
      terms.effective_leverage_capped = spread_scale.clone();

      if amm.base_asset_amount_with_amm.gt(&0) {
        long_spread = long_spread.clone().mul(&spread_scale);
      } else {
        short_spread = short_spread.clone().mul(&spread_scale);
      }
    } else {
      long_spread = long_spread.clone().mul(&max_spread_scale);
      short_spread = short_spread.clone().mul(&max_spread_scale);
    }

    terms.long_spread_w_el = long_spread.clone();
    terms.short_spread_w_el = short_spread.clone();

    if amm
      .net_revenue_since_last_funding
      .lt(&DEFAULT_REVENUE_SINCE_LAST_FUNDING_SPREAD_RETREAT)
    {
      let max_retreat = max_target_spread.clone().div(BigInt::from(10));
      let mut revenue_retreat_amount = max_retreat.clone();
      if amm
        .net_revenue_since_last_funding
        .ge(&DEFAULT_REVENUE_SINCE_LAST_FUNDING_SPREAD_RETREAT.mul(1_000))
      {
        revenue_retreat_amount = max_retreat.clone().min(
          BigInt::from(amm.base_spread)
            .mul(BigInt::from(amm.net_revenue_since_last_funding.abs()))
            .div(BigInt::from(
              DEFAULT_REVENUE_SINCE_LAST_FUNDING_SPREAD_RETREAT.abs(),
            )),
        );
      }

      let half_revenue_retreat_amount = revenue_retreat_amount.clone().div(BigInt::from(2));

      terms.revenue_retreat_amount = revenue_retreat_amount.clone();
      terms.half_revenue_retreat_amount = half_revenue_retreat_amount.clone();

      if amm.base_asset_amount_with_amm.gt(&0) {
        long_spread = long_spread.clone().add(&revenue_retreat_amount);
        short_spread = short_spread.clone().add(&half_revenue_retreat_amount);
      } else if amm.base_asset_amount_with_amm.lt(&0) {
        long_spread = long_spread.clone().add(&half_revenue_retreat_amount);
        short_spread = short_spread.clone().add(&revenue_retreat_amount);
      } else {
        long_spread = long_spread.clone().add(&half_revenue_retreat_amount);
        short_spread = short_spread.clone().add(&half_revenue_retreat_amount);
      }
    }

    terms.long_spread_w_rev_retreat = long_spread.clone();
    terms.short_spread_w_rev_retreat = short_spread.clone();

    let total_spread = long_spread.clone().add(&short_spread);
    if total_spread.gt(&max_target_spread) {
      if long_spread > short_spread {
        long_spread = long_spread
          .clone()
          .mul(&max_target_spread)
          .div(&total_spread);
        short_spread = max_target_spread.clone().sub(&long_spread);
      } else {
        short_spread = short_spread
          .clone()
          .mul(&max_target_spread)
          .div(&total_spread);
        long_spread = max_target_spread.clone().sub(&short_spread);
      }
    }

    terms.total_spread = total_spread;
    terms.long_spread = long_spread.clone();
    terms.short_spread = short_spread.clone();

    Spread {
      long_spread,
      short_spread,
    }
  }

  fn calc_vol_spread(
    last_oracle_conf_pct: BigInt,
    reserve_price: BigInt,
    mark_std: BigInt,
    oracle_std: BigInt,
    long_intensity: BigInt,
    short_intensity: BigInt,
    volume_24h: BigInt,
  ) -> VolSpread {
    let market_avg_std_pct = mark_std
      .add(oracle_std)
      .mul(PERCENTAGE_PRECISION_U64)
      .div(reserve_price)
      .div(BigInt::from(2));
    let vol_spread = last_oracle_conf_pct.clone().max(market_avg_std_pct.div(2));

    let clamp_min = BigInt::from(PERCENTAGE_PRECISION.div(100));
    let clamp_max = BigInt::from(PERCENTAGE_PRECISION.mul(16).div(10));

    let long_vol_spread_factor = Self::clamp_num(
      long_intensity
        .mul(BigInt::from(PERCENTAGE_PRECISION))
        .div(BigInt::from(1).max(volume_24h.clone())),
      clamp_min.clone(),
      clamp_max.clone(),
    );

    let short_vol_spread_factor = Self::clamp_num(
      short_intensity
        .mul(PERCENTAGE_PRECISION)
        .div(BigInt::from(1).max(volume_24h)),
      clamp_min,
      clamp_max,
    );

    let conf_component = if last_oracle_conf_pct.le(&BigInt::from(PRICE_PRECISION).div(400)) {
      last_oracle_conf_pct.div(10)
    } else {
      last_oracle_conf_pct.clone()
    };

    let long_vol_spread = conf_component.clone().max(
      vol_spread
        .clone()
        .mul(long_vol_spread_factor)
        .div(PERCENTAGE_PRECISION),
    );
    let short_vol_spread = conf_component.clone().max(
      vol_spread
        .mul(short_vol_spread_factor)
        .div(PERCENTAGE_PRECISION),
    );
    VolSpread {
      long_vol_spread,
      short_vol_spread,
    }
  }

  fn calc_effective_leverage(amm: &AMM, reserve_price: BigInt) -> BigInt {
    let net_base_asset_amount = BigInt::from(amm.base_asset_amount_with_amm);
    let terminal_quote_asset_reserve = BigInt::from(amm.terminal_quote_asset_reserve);
    let peg_multiplier = BigInt::from(amm.peg_multiplier);
    let net_base_asset_value = BigInt::from(amm.quote_asset_reserve)
      .sub(terminal_quote_asset_reserve)
      .mul(peg_multiplier)
      .div(AMM_TIMES_PEG_TO_QUOTE_PRECISION_RATIO);
    let local_base_asset_value = net_base_asset_amount
      .mul(reserve_price)
      .div(AMM_TO_QUOTE_PRECISION_RATIO.mul(PRICE_PRECISION));
    let effective_gap = BigInt::from(0).max((local_base_asset_value).sub(net_base_asset_value));
    let effective_leverage =
      effective_gap / (0.max(amm.total_fee_minus_distributions) + 1) + 1 / QUOTE_PRECISION;
    effective_leverage
  }

  fn calc_max_spread(margin_ratio_init: u32) -> u32 {
    margin_ratio_init.mul(BID_ASK_SPREAD_PRECISION.div(MARGIN_PRECISION as u64) as u32)
  }

  fn calc_inventory_scale(
    base_asset_amount_with_amm: BigInt,
    base_asset_reserve: BigInt,
    min_base_asset_reserve: BigInt,
    max_base_asset_reserve: BigInt,
    dir_spread: BigInt,
    max_spread: BigInt,
  ) -> BigInt {
    if base_asset_amount_with_amm.eq(&BigInt::from(0)) {
      return BigInt::from(1);
    }
    let max_bid_ask_inventory_skew_factor: BigInt = BigInt::from(BID_ASK_SPREAD_PRECISION * 10);
    let inventory_scale = Self::calc_inventory_liquidity_ratio(
      base_asset_amount_with_amm,
      base_asset_reserve,
      min_base_asset_reserve,
      max_base_asset_reserve,
    );

    let inventory_scale_max = max_bid_ask_inventory_skew_factor.max(
      BigInt::from(max_spread)
        .mul(BigInt::from(BID_ASK_SPREAD_PRECISION))
        .div((dir_spread).max(BigInt::from(1))),
    );
    let inventory_scale_capped = inventory_scale_max
      .clone()
      .min(
        BID_ASK_SPREAD_PRECISION.add(
          inventory_scale_max
            .clone()
            .mul(inventory_scale)
            .div(PERCENTAGE_PRECISION as u64),
        ),
      )
      .div(BID_ASK_SPREAD_PRECISION);
    inventory_scale_capped
  }

  fn new_oracle_conf_pct(
    amm: &AMM,
    oracle: OraclePriceData,
    reserve_price: BigInt,
    now: i64,
  ) -> BigInt {
    let conf_interval = BigInt::from(oracle.confidence);
    let since_last_update = 0.max(now.sub(amm.historical_oracle_data.last_oracle_price_twap_ts));
    let mut lower_bound_conf_pct = BigInt::from(amm.last_oracle_conf_pct);
    if since_last_update.gt(&0) {
      let lower_bound_conf_divisor = BigInt::from(21.sub(since_last_update).max(5));
      lower_bound_conf_pct = BigInt::from(amm.last_oracle_conf_pct).sub(BigInt::from(
        amm.last_oracle_conf_pct.div(lower_bound_conf_divisor),
      ));
    }
    let conf_interval_pct = conf_interval
      .mul(BID_ASK_SPREAD_PRECISION)
      .div(reserve_price);
    let conf_interval_pct_result = conf_interval_pct.max(lower_bound_conf_pct);
    conf_interval_pct_result
  }

  fn calc_live_oracle_std(amm: &AMM, oracle: OraclePriceData, now: i64) -> BigInt {
    let since_last_update = 1.max(now.sub(amm.historical_oracle_data.last_oracle_price_twap_ts));
    let since_start = 0.max(amm.funding_period.sub(since_last_update));
    let live_oracle_twap =
      Self::calc_live_oracle_twap(amm.historical_oracle_data, oracle, now, amm.funding_period);
    let price_delta_vs_twap = BigInt::from(oracle.price.sub(live_oracle_twap).abs());
    let oracle_std = price_delta_vs_twap.add(
      BigInt::from(amm.oracle_std)
        .mul(since_start)
        .div(since_start.add(since_last_update)),
    );
    oracle_std
  }

  fn calc_live_oracle_twap(
    hist_oracle_data: HistoricalOracleData,
    oracle: OraclePriceData,
    now: i64,
    period: i64,
  ) -> i64 {
    let oracle_twap = if period.eq(&(FIVE_MINUTE as i64)) {
      hist_oracle_data.last_oracle_price_twap5min
    } else {
      hist_oracle_data.last_oracle_price_twap
    };
    let since_last_update = 1.max(now.sub(hist_oracle_data.last_oracle_price_twap_ts));
    let since_start = 0.max(period.sub(since_last_update));
    let clamp_range = oracle_twap.div(3);
    let clamped_oracle_price = oracle_twap
      .add(clamp_range)
      .min(oracle.price.max(oracle_twap.sub(clamp_range)));
    let new_oracle_twap = oracle_twap
      .mul(since_start)
      .add(clamped_oracle_price.mul(since_last_update))
      .div(since_start.add(since_last_update));
    new_oracle_twap
  }

  fn calc_reference_point_offset(
    amm: &AMM,
    reserve_price: BigInt,
    liquidity_fraction: BigInt,
    max_offset_pct: BigInt,
  ) -> BigInt {
    let last_24h_avg_funding_rate = BigInt::from(amm.last24h_avg_funding_rate);
    let oracle_twap_fast = BigInt::from(amm.historical_oracle_data.last_oracle_price_twap5min);
    let mark_twap_fast = BigInt::from(amm.last_mark_price_twap5min);
    let oracle_twap_slow = BigInt::from(amm.historical_oracle_data.last_oracle_price_twap);
    let mark_twap_slow = BigInt::from(amm.last_mark_price_twap);

    if last_24h_avg_funding_rate.eq(&BigInt::from(0)) {
      return BigInt::from(0);
    }
    let max_offset_in_price = max_offset_pct
      .clone()
      .mul(reserve_price)
      .div(BigInt::from(PERCENTAGE_PRECISION));
    let mark_premium_minute = Self::clamp_num(
      mark_twap_fast.sub(oracle_twap_fast),
      max_offset_in_price.clone().mul(-1),
      max_offset_in_price.clone(),
    );
    let mark_premium_hour = Self::clamp_num(
      mark_twap_slow.sub(oracle_twap_slow),
      max_offset_in_price.clone().mul(-1),
      max_offset_in_price.clone(),
    );
    let mark_premium_day = Self::clamp_num(
      last_24h_avg_funding_rate
        .div(FUNDING_RATE_BUFFER_PRECISION)
        .mul(24),
      max_offset_in_price.clone().mul(-1),
      max_offset_in_price.clone(),
    );
    let mark_premium_avg_pct = mark_premium_minute
      .add(mark_premium_hour)
      .add(mark_premium_day)
      .div(BigInt::from(3));
    let inventory_pct = Self::clamp_num(
      liquidity_fraction
        .mul(max_offset_pct)
        .div(PERCENTAGE_PRECISION),
      max_offset_in_price.clone().mul(-1),
      max_offset_in_price.clone(),
    );
    let mut offset_pct = mark_premium_avg_pct.clone().add(&inventory_pct);
    if !Self::sig_num(inventory_pct).eq(&Self::sig_num(mark_premium_avg_pct)) {
      offset_pct = BigInt::from(0);
    }
    let clamped_offset_pct = Self::clamp_num(
      offset_pct,
      max_offset_in_price.clone().mul(-1),
      max_offset_in_price,
    );
    clamped_offset_pct
  }

  fn clamp_num(x: BigInt, min: BigInt, max: BigInt) -> BigInt {
    min.max(x.min(max))
  }

  fn sig_num(x: BigInt) -> i8 {
    if x.lt(&BigInt::from(0)) {
      -1
    } else {
      1
    }
  }

  fn calc_inventory_liquidity_ratio(
    base_asset_amount_with_amm: BigInt,
    base_asset_reserve: BigInt,
    min_base_asset_reserve: BigInt,
    max_base_asset_reserve: BigInt,
  ) -> BigInt {
    let OpenBidAsk {
      open_bids,
      open_asks,
    } = Self::calc_market_open_bid_ask(
      base_asset_reserve,
      min_base_asset_reserve,
      max_base_asset_reserve,
      None,
    );
    let min_side_liquidity = open_bids.abs().min(open_asks.abs());
    let inventory_scale = BigInt::from(PERCENTAGE_PRECISION).min(
      BigInt::from(base_asset_amount_with_amm)
        .mul(BigInt::from(PERCENTAGE_PRECISION))
        .div(BigInt::from(min_side_liquidity).max(BigInt::from(1)))
        .abs(),
    );
    inventory_scale
  }

  fn calc_market_open_bid_ask(
    base_asset_reserve: BigInt,
    min_base_asset_reserve: BigInt,
    max_base_asset_reserve: BigInt,
    step_size: Option<BigInt>,
  ) -> OpenBidAsk {
    let open_asks = if min_base_asset_reserve.lt(&base_asset_reserve) {
      let mut _open_asks = base_asset_reserve
        .clone()
        .sub(min_base_asset_reserve)
        .mul(BigInt::from(-1));
      if let Some(step_size) = &step_size {
        if _open_asks.abs().div(BigInt::from(2)).lt(step_size) {
          _open_asks = BigInt::from(0);
        }
      }
      _open_asks
    } else {
      BigInt::from(0)
    };

    let open_bids = if max_base_asset_reserve.gt(&base_asset_reserve) {
      let mut _open_bids = max_base_asset_reserve.sub(&base_asset_reserve);
      if let Some(step_size) = step_size.clone() {
        if _open_bids
          .clone()
          .div(BigInt::from(2))
          .lt(&BigInt::from(step_size))
        {
          _open_bids = BigInt::from(0);
        }
      }
      _open_bids
    } else {
      BigInt::from(0)
    };
    OpenBidAsk {
      open_bids,
      open_asks,
    }
  }

  fn calc_budgeted_peg(amm: &AMM, budget: BigInt, target_price: BigInt) -> BigInt {
    let mut per_peg_cost = amm
      .quote_asset_reserve
      .sub(amm.terminal_quote_asset_reserve)
      .div(AMM_RESERVE_PRECISION.div(PRICE_PRECISION));

    if per_peg_cost.gt(&0) {
      per_peg_cost = per_peg_cost.add(1);
    } else if per_peg_cost.lt(&0) {
      per_peg_cost = per_peg_cost.sub(1);
    }

    let target_peg = target_price
      .mul(amm.base_asset_reserve)
      .div(amm.quote_asset_reserve)
      .div(PRICE_DIV_PEG);

    let peg_change_dir = target_peg.clone().sub(amm.peg_multiplier);
    let use_target_peg = (per_peg_cost.lt(&0) && peg_change_dir.gt(&BigInt::from(0)))
      || (per_peg_cost.gt(&0) && peg_change_dir.lt(&BigInt::from(0)));

    if per_peg_cost.eq(&0) || use_target_peg {
      return target_peg;
    }
    let budget_delta_peg = budget.mul(PEG_PRECISION).div(per_peg_cost);
    let new_peg = amm
      .peg_multiplier
      .add(budget_delta_peg)
      .max(BigInt::from(1));
    new_peg
  }

  fn calc_adjust_k_cost(amm: &AMM, num: BigInt, denom: BigInt) -> BigInt {
    let x = BigInt::from(amm.base_asset_reserve);
    let y = BigInt::from(amm.quote_asset_reserve);
    let d = BigInt::from(amm.base_asset_amount_with_amm);
    let q = BigInt::from(amm.peg_multiplier);
    let quote_scale = y.mul(&d).mul(&q);
    let p = BigInt::from(num.mul(PRICE_PRECISION_I128).div(denom));

    quote_scale
      .clone()
      .mul(PERCENTAGE_PRECISION_I128)
      .mul(PERCENTAGE_PRECISION_I128)
      .div(x.clone().add(d.clone()))
      .sub(
        quote_scale
          .mul(&p)
          .mul(PERCENTAGE_PRECISION_I128)
          .mul(PERCENTAGE_PRECISION_I128)
          .div(PRICE_PRECISION_I128)
          .div(x.clone().mul(p).div(PRICE_PRECISION_I128).add(d)),
      )
      .div(PERCENTAGE_PRECISION_I128)
      .div(PERCENTAGE_PRECISION_I128)
      .div(AMM_TO_QUOTE_PRECISION_RATIO_I128)
      .div(PEG_PRECISION_I128)
      .mul(-1)
  }

  fn swap_direction(asset_type: AssetType, dir: PositionDirection) -> SwapDirection {
    if matches!(dir, PositionDirection::Long) && matches!(asset_type, AssetType::Base)
      || matches!(dir, PositionDirection::Short) && matches!(asset_type, AssetType::Quote)
    {
      SwapDirection::Remove
    } else {
      SwapDirection::Add
    }
  }

  fn calc_amm_reserves_after_swap(
    amm: &AMM,
    input_asset_type: AssetType,
    swap_amount: BigInt,
    swap_direction: SwapDirection,
  ) -> AmmReservesAfterSwap {
    assert!(swap_amount.ge(&BigInt::from(0)));
    if matches!(input_asset_type, AssetType::Quote) {
      Self::calc_swap_output(
        BigInt::from(amm.quote_asset_reserve),
        swap_amount
          .mul(BigInt::from(AMM_TIMES_PEG_TO_QUOTE_PRECISION_RATIO))
          .div(BigInt::from(amm.peg_multiplier)),
        swap_direction,
        BigInt::from(amm.sqrt_k * amm.sqrt_k),
      )
    } else {
      Self::calc_swap_output(
        BigInt::from(amm.base_asset_reserve),
        swap_amount,
        swap_direction,
        BigInt::from(amm.sqrt_k * amm.sqrt_k),
      )
    }
  }

  fn calc_swap_output(
    input_asset_reserve: BigInt,
    swap_amount: BigInt,
    swap_direction: SwapDirection,
    invariant: BigInt,
  ) -> AmmReservesAfterSwap {
    let new_input_asset_reserve = if matches!(swap_direction, SwapDirection::Add) {
      input_asset_reserve + swap_amount
    } else {
      input_asset_reserve - swap_amount
    };
    let new_output_asset_reserve = invariant.div(&new_input_asset_reserve);
    AmmReservesAfterSwap {
      new_quote_asset_reserve: new_input_asset_reserve,
      new_base_asset_reserve: new_output_asset_reserve,
    }
  }

  fn calc_optimal_peg_and_budget(amm: &AMM, oracle: OraclePriceData) -> OptimalPegAndBudget {
    let reserve_price_before = Self::calc_price(
      BigInt::from(amm.base_asset_reserve),
      BigInt::from(amm.quote_asset_reserve),
      BigInt::from(amm.peg_multiplier),
    );
    let target_price = oracle.price;
    let new_peg = Self::calc_peg_from_target_price(
      BigInt::from(target_price),
      BigInt::from(amm.base_asset_reserve),
      BigInt::from(amm.quote_asset_reserve),
    );
    let pre_peg_cost = Self::calc_repeg_cost(amm, new_peg.clone());
    let total_fee_lb = BigInt::from(amm.total_exchange_fee).div(2);
    let budget = BigInt::from(amm.total_fee_minus_distributions)
      .sub(BigInt::from(total_fee_lb))
      .max(BigInt::from(0));

    let mut check_lower_bound = true;
    if budget < pre_peg_cost {
      let half_max_price_spread = BigInt::from(amm.max_spread)
        .div(BigInt::from(2))
        .mul(BigInt::from(target_price))
        .div(BigInt::from(BID_ASK_SPREAD_PRECISION));
      let target_price_gap = reserve_price_before.clone().sub(target_price);

      if target_price_gap.abs() > half_max_price_spread {
        let mark_adj = (target_price_gap.abs()).sub(BigInt::from(half_max_price_spread));
        let new_target_price = if target_price_gap.lt(&BigInt::from(0)) {
          reserve_price_before.add(mark_adj)
        } else {
          reserve_price_before.sub(mark_adj)
        };

        let new_optimal_peg = Self::calc_peg_from_target_price(
          new_target_price.clone(),
          BigInt::from(amm.base_asset_reserve),
          BigInt::from(amm.quote_asset_reserve),
        );

        let new_budget = Self::calc_repeg_cost(amm, new_optimal_peg.clone());
        check_lower_bound = false;
        return OptimalPegAndBudget {
          target_price: new_target_price,
          new_peg: new_optimal_peg,
          budget: new_budget,
          check_lower_bound,
        };
      } else if amm
        .total_fee_minus_distributions
        .lt(&(amm.total_exchange_fee.div(2) as i128))
      {
        check_lower_bound = false;
      }
    }

    OptimalPegAndBudget {
      target_price: BigInt::from(target_price),
      new_peg: BigInt::from(new_peg),
      budget,
      check_lower_bound,
    }
  }

  fn calc_peg_from_target_price(
    target_price: BigInt,
    base_asset_reserve: BigInt,
    quote_asset_reserve: BigInt,
  ) -> BigInt {
    BigInt::from(target_price)
      .mul(base_asset_reserve)
      .div(quote_asset_reserve)
      .add((PRICE_DIV_PEG).div(2))
      .div(PRICE_DIV_PEG)
      .max(BigInt::from(1))
  }

  fn calc_repeg_cost(amm: &AMM, new_peg: BigInt) -> BigInt {
    let dqar =
      BigInt::from(amm.quote_asset_reserve).sub(BigInt::from(amm.terminal_quote_asset_reserve));
    dqar
      .mul(BigInt::from(new_peg).sub(BigInt::from(amm.peg_multiplier)))
      .div(BigInt::from(AMM_TO_QUOTE_PRECISION_RATIO))
      .div(BigInt::from(PEG_PRECISION))
  }

  fn calc_price(
    base_asset_reserve: BigInt,
    quote_asset_reserve: BigInt,
    peg_multiplier: BigInt,
  ) -> BigInt {
    BigInt::from(quote_asset_reserve)
      .mul(BigInt::from(PRICE_PRECISION))
      .mul(BigInt::from(peg_multiplier))
      .div(BigInt::from(PEG_PRECISION))
      .div(BigInt::from(base_asset_reserve))
  }
}
