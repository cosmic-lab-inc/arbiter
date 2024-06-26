use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use anchor_lang::prelude::{AccountInfo, AccountMeta};
use num_bigint::BigInt;
use solana_sdk::pubkey::Pubkey;

use crate::drift_client::{DriftUtils, ReadCache};
use crate::Time;
use drift_cpi::{
  MarketType, OraclePriceData, OracleSource, Order, OrderStatus, OrderType, PerpMarket,
  PerpPosition, PositionDirection, SpotMarket, User, BASE_PRECISION, PRICE_PRECISION,
};

#[derive(Clone)]
pub struct RemainingAccountParams {
  pub user_accounts: Vec<User>,
  pub readable_perp_market_indexes: Option<Vec<u16>>,
  pub writable_perp_market_indexes: Option<Vec<u16>>,
  pub readable_spot_market_indexes: Option<Vec<u16>>,
  pub writable_spot_market_indexes: Option<Vec<u16>>,
  pub use_market_last_slot_cache: bool,
}

#[derive(Debug, Clone)]
pub struct RemainingAccountMaps {
  pub oracle_account_map: HashMap<String, AccountInfo<'static>>,
  pub spot_market_account_map: HashMap<u16, AccountInfo<'static>>,
  pub perp_market_account_map: HashMap<u16, AccountInfo<'static>>,
}

#[derive(Debug, Clone)]
pub struct MarketMetadata {
  pub perp_oracle: Pubkey,
  pub perp_oracle_source: OracleSource,
  pub perp_oracle_price_data: Option<OraclePriceData>,
  pub spot_oracle: Pubkey,
  pub spot_oracle_source: OracleSource,
  pub spot_oracle_price_data: Option<OraclePriceData>,
  pub perp_name: String,
  pub perp_market_index: u16,
  pub spot_market_index: u16,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct MarketId {
  pub index: u16,
  pub kind: MarketType,
}

impl PartialEq<Self> for MarketId {
  fn eq(&self, other: &Self) -> bool {
    let index_eq = self.index == other.index;
    let kind_eq = matches!(
      (self.kind, other.kind),
      (MarketType::Spot, MarketType::Spot) | (MarketType::Perp, MarketType::Perp)
    );
    index_eq && kind_eq
  }
}

impl Eq for MarketId {}

impl Hash for MarketId {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.index.hash(state);
    let kind = match self.kind {
      MarketType::Spot => 0,
      MarketType::Perp => 1,
    };
    kind.hash(state);
  }
}

impl MarketId {
  pub fn key(&self) -> Pubkey {
    match self.kind {
      MarketType::Spot => DriftUtils::spot_market_pda(self.index),
      MarketType::Perp => DriftUtils::perp_market_pda(self.index),
    }
  }

  /// Id of a perp market
  pub const fn perp(index: u16) -> Self {
    Self {
      index,
      kind: MarketType::Perp,
    }
  }
  /// Id of a spot market
  pub const fn spot(index: u16) -> Self {
    Self {
      index,
      kind: MarketType::Spot,
    }
  }

  /// `MarketId` for the USDC Spot Market
  pub const QUOTE_SPOT: Self = Self {
    index: 0,
    kind: MarketType::Spot,
  };

  pub const SOL_PERP: Self = Self {
    index: 0,
    kind: MarketType::Perp,
  };

  pub fn kind_eq(&self, other: MarketType) -> bool {
    matches!(
      (self.kind, other),
      (MarketType::Spot, MarketType::Spot) | (MarketType::Perp, MarketType::Perp)
    )
  }
}

impl From<(u16, MarketType)> for MarketId {
  fn from(value: (u16, MarketType)) -> Self {
    Self {
      index: value.0,
      kind: value.1,
    }
  }
}

/// Helper type for Accounts included in drift instructions
///
/// Provides sorting implementation matching drift program
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RemainingAccount {
  Oracle { pubkey: Pubkey },
  Spot { pubkey: Pubkey, writable: bool },
  Perp { pubkey: Pubkey, writable: bool },
}

impl RemainingAccount {
  fn pubkey(&self) -> &Pubkey {
    match self {
      Self::Oracle { pubkey } => pubkey,
      Self::Spot { pubkey, .. } => pubkey,
      Self::Perp { pubkey, .. } => pubkey,
    }
  }
  fn parts(self) -> (Pubkey, bool) {
    match self {
      Self::Oracle { pubkey } => (pubkey, false),
      Self::Spot {
        pubkey, writable, ..
      } => (pubkey, writable),
      Self::Perp {
        pubkey, writable, ..
      } => (pubkey, writable),
    }
  }
  fn discriminant(&self) -> u8 {
    // SAFETY: Because `Self` is marked `repr(u8)`, its layout is a `repr(C)` `union`
    // between `repr(C)` structs, each of which has the `u8` discriminant as its first
    // field, so we can read the discriminant without offsetting the pointer.
    // unsafe { *<*const _>::from(self).cast::<u8>() }

    // get discriminant of self enum
    let discrim: u8 = match self {
      Self::Oracle { .. } => 0,
      Self::Spot { .. } => 1,
      Self::Perp { .. } => 2,
    };
    discrim
  }
}

impl Ord for RemainingAccount {
  fn cmp(&self, other: &Self) -> Ordering {
    // let type_order = self.cmp(&other);
    let type_order = self.discriminant().cmp(&other.discriminant());
    if let Ordering::Equal = type_order {
      self.pubkey().cmp(other.pubkey())
    } else {
      type_order
    }
  }
}

impl PartialOrd for RemainingAccount {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl From<RemainingAccount> for AccountMeta {
  fn from(value: RemainingAccount) -> Self {
    let (pubkey, is_writable) = value.parts();
    AccountMeta {
      pubkey,
      is_writable,
      is_signer: false,
    }
  }
}

pub struct DlobNode {
  pub order: Order,
  pub user: Pubkey,
}

impl PartialEq for DlobNode {
  fn eq(&self, other: &Self) -> bool {
    // check if direction enums match
    let same_dir = (matches!(self.order.direction, PositionDirection::Long)
      && matches!(other.order.direction, PositionDirection::Long))
      || (matches!(self.order.direction, PositionDirection::Short)
        && matches!(other.order.direction, PositionDirection::Short));
    let same_id = self.order.order_id == other.order.order_id;
    let same_market_type = (matches!(self.order.market_type, MarketType::Perp)
      && matches!(other.order.market_type, MarketType::Perp))
      || (matches!(self.order.market_type, MarketType::Spot)
        && matches!(other.order.market_type, MarketType::Spot));
    let same_market_index = self.order.market_index == other.order.market_index;
    same_dir && same_id && same_market_type && same_market_index
  }
}
impl Eq for DlobNode {}
impl Hash for DlobNode {
  fn hash<H: Hasher>(&self, state: &mut H) {
    match self.order.direction {
      PositionDirection::Long => 0.hash(state),
      PositionDirection::Short => 1.hash(state),
    };
    match self.order.market_type {
      MarketType::Perp => 0.hash(state),
      MarketType::Spot => 1.hash(state),
    };
    self.order.market_index.hash(state);
    self.order.order_id.hash(state);
    self.order.base_asset_amount.hash(state);
  }
}

impl DlobNode {
  pub fn new(user: Pubkey, order: Order) -> Self {
    Self { user, order }
  }

  pub fn price(&self, cache: &ReadCache<'_>) -> anyhow::Result<f64> {
    Ok(match self.order.price == 0 {
      true => {
        let market = MarketId::from((self.order.market_index, self.order.market_type));
        let oracle_price = DriftUtils::oracle_price(&market, cache, Some(self.order.slot))?;
        let offset = self.order.oracle_price_offset as f64 / PRICE_PRECISION as f64;
        oracle_price + offset
      }
      false => self.order.price as f64 / PRICE_PRECISION as f64,
    })
  }

  pub fn slot(&self) -> u64 {
    self.order.slot
  }

  pub fn size(&self) -> f64 {
    (self.order.base_asset_amount - self.order.base_asset_amount_filled) as f64
      / BASE_PRECISION as f64
  }

  pub fn filled(&self) -> bool {
    self.order.base_asset_amount == self.order.base_asset_amount_filled
      || matches!(self.order.status, OrderStatus::Filled)
  }

  pub fn canceled(&self) -> bool {
    matches!(self.order.status, OrderStatus::Canceled)
  }

  pub fn expired(&self) -> bool {
    Time::now().to_unix() > self.order.max_ts && self.order.max_ts != 0
  }

  pub fn base(&self) -> f64 {
    self.order.base_asset_amount as f64 / BASE_PRECISION as f64
  }

  pub fn filled_base(&self) -> f64 {
    self.order.base_asset_amount_filled as f64 / BASE_PRECISION as f64
  }

  pub fn is_bid(&self) -> bool {
    matches!(self.order.direction, PositionDirection::Long)
  }

  pub fn is_valid(&self) -> bool {
    !self.filled() && !self.canceled() && !self.expired()
  }

  pub fn is_ask(&self) -> bool {
    matches!(self.order.direction, PositionDirection::Short)
  }

  pub fn bid(&self, cache: &ReadCache<'_>) -> anyhow::Result<OrderInfo> {
    if !self.is_bid() {
      return Err(anyhow::anyhow!("Order is not a bid"));
    }
    Ok(OrderInfo {
      price: self.price(cache)?,
      size: self.size(),
      slot: self.slot(),
      user: self.user,
      order: self.order,
    })
  }

  pub fn ask(&self, cache: &ReadCache<'_>) -> anyhow::Result<OrderInfo> {
    if !self.is_ask() {
      return Err(anyhow::anyhow!("Order is not an ask"));
    }
    Ok(OrderInfo {
      price: self.price(cache)?,
      size: self.size(),
      slot: self.slot(),
      user: self.user,
      order: self.order,
    })
  }
}

pub struct OrderInfo {
  pub price: f64,
  pub size: f64,
  pub slot: u64,
  pub user: Pubkey,
  pub order: Order,
}

pub struct L3Orderbook {
  /// First index is highest/best bid
  pub bids: Vec<OrderInfo>,
  /// First index is lowest/best ask
  pub asks: Vec<OrderInfo>,
  pub spread: f64,
  pub slot: u64,
  pub oracle_price: f64,
  pub last_price: f64,
}
impl L3Orderbook {
  pub fn best_bid(&self) -> anyhow::Result<&OrderInfo> {
    self
      .bids
      .iter()
      .filter(|o| o.price < self.oracle_price)
      .max_by(|a, b| a.price.partial_cmp(&b.price).unwrap())
      .ok_or(anyhow::anyhow!("No bids"))
  }

  pub fn best_ask(&self) -> anyhow::Result<&OrderInfo> {
    self
      .asks
      .iter()
      .filter(|o| o.price > self.oracle_price)
      .min_by(|a, b| a.price.partial_cmp(&b.price).unwrap())
      .ok_or(anyhow::anyhow!("No asks"))
  }

  pub fn uncross_asks(
    &self,
    pct_cutoff: f64,
    oracle_price: f64,
    slot: u64,
  ) -> anyhow::Result<Vec<&OrderInfo>> {
    assert!(pct_cutoff >= 0.0);
    let quote_cutoff = self.oracle_price * (1.0 - pct_cutoff / 100.0);
    let mut asks = self
      .asks
      .iter()
      .flat_map(|o| {
        let a = !matches!(o.order.status, OrderStatus::Open);
        let b = !matches!(o.order.market_type, MarketType::Perp);
        let c = !DriftUtils::order_is_limit(&o.order)
          || (DriftUtils::order_must_be_triggered(&o.order)
            && !DriftUtils::order_triggered(&o.order));
        let d = !DriftUtils::order_is_resting_limit(&o.order, slot)?;
        let e = Time::now().to_unix() > o.order.max_ts && o.order.max_ts != 0;
        let f = !(o.price < oracle_price && o.price >= quote_cutoff);
        if a || b || c || d || e || f {
          Err(anyhow::anyhow!("Invalid order"))
        } else {
          Ok(o)
        }
      })
      .collect::<Vec<&OrderInfo>>();
    // sort so the lowest price is first
    asks.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());
    Ok(asks)
  }

  pub fn uncross_bids(
    &self,
    pct_cutoff: f64,
    oracle_price: f64,
    slot: u64,
  ) -> anyhow::Result<Vec<&OrderInfo>> {
    assert!(pct_cutoff >= 0.0);
    let quote_cutoff = oracle_price * (1.0 + pct_cutoff / 100.0);
    let mut bids = self
      .bids
      .iter()
      .flat_map(|o| {
        let a = !matches!(o.order.status, OrderStatus::Open);
        let b = !matches!(o.order.market_type, MarketType::Perp);
        let c = !DriftUtils::order_is_limit(&o.order)
          || (DriftUtils::order_must_be_triggered(&o.order)
            && !DriftUtils::order_triggered(&o.order));
        let d = !DriftUtils::order_is_resting_limit(&o.order, slot)?;
        let now = Time::now().to_unix();
        let e = now > o.order.max_ts && o.order.max_ts != 0;
        let f = !(o.price > self.oracle_price && o.price <= quote_cutoff);
        if a || b || c || d || e || f {
          Err(anyhow::anyhow!("Invalid order"))
        } else {
          Ok(o)
        }
      })
      .collect::<Vec<&OrderInfo>>();
    // sort so the highest price is first
    bids.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap());
    Ok(bids)
  }

  /// Find maker asks below the pct_cutoff.
  /// This is used to take profit for a short position
  /// by placing a taker bid against this maker ask.
  pub fn take_profit_maker_asks(
    &self,
    spos: &PerpPosition,
    pct_cutoff: f64,
    slot: u64,
  ) -> anyhow::Result<Option<&OrderInfo>> {
    // can only take profit long if the position is short
    if spos.base_asset_amount > 0 {
      return Ok(None);
    }
    let perp_entry = DriftUtils::perp_position_price(spos);
    let quote_cutoff = perp_entry * (1.0 - pct_cutoff / 100.0);
    let asks = self
      .asks
      .iter()
      .flat_map(|o| {
        let a = !matches!(o.order.status, OrderStatus::Open);
        let b = !matches!(o.order.market_type, MarketType::Perp);
        let c = !DriftUtils::order_is_limit(&o.order)
          || (DriftUtils::order_must_be_triggered(&o.order)
            && !DriftUtils::order_triggered(&o.order));
        let d = !DriftUtils::order_is_resting_limit(&o.order, slot)?;
        let e = Time::now().to_unix() > o.order.max_ts && o.order.max_ts != 0;
        let f = o.price > quote_cutoff;
        if a || b || c || d || e || f {
          Err(anyhow::anyhow!("Invalid order"))
        } else {
          Ok(o)
        }
      })
      .collect::<Vec<&OrderInfo>>();
    // get ask with the lowest price
    let lowest_ask = asks
      .into_iter()
      .min_by(|a, b| a.price.partial_cmp(&b.price).unwrap());
    Ok(lowest_ask)
  }

  /// Find maker bids above the pct_cutoff.
  /// This is used to take profit for a long position
  /// by placing a taker ask against this maker bid.
  pub fn take_profit_maker_bids(
    &self,
    lpos: &PerpPosition,
    pct_cutoff: f64,
    slot: u64,
  ) -> anyhow::Result<Option<&OrderInfo>> {
    // can only take profit short if the position is long
    if lpos.base_asset_amount < 0 {
      return Ok(None);
    }
    let perp_entry = DriftUtils::perp_position_price(lpos);
    let quote_cutoff = perp_entry * (1.0 + pct_cutoff / 100.0);
    let bids = self
      .bids
      .iter()
      .flat_map(|o| {
        let a = !matches!(o.order.status, OrderStatus::Open);
        let b = !matches!(o.order.market_type, MarketType::Perp);
        let c = !DriftUtils::order_is_limit(&o.order)
          || (DriftUtils::order_must_be_triggered(&o.order)
            && !DriftUtils::order_triggered(&o.order));
        let d = !DriftUtils::order_is_resting_limit(&o.order, slot)?;
        let e = Time::now().to_unix() > o.order.max_ts && o.order.max_ts != 0;
        let f = o.price < quote_cutoff;
        if a || b || c || d || e || f {
          Err(anyhow::anyhow!("Invalid order"))
        } else {
          Ok(o)
        }
      })
      .collect::<Vec<&OrderInfo>>();
    // get bids with the highest price
    let highest_bid = bids
      .into_iter()
      .max_by(|a, b| a.price.partial_cmp(&b.price).unwrap());
    Ok(highest_bid)
  }
}

pub enum OrderPriceType {
  Price(f64),
  OraclePercentOffset(f64),
}

pub struct OrderBuilder {
  pub market: MarketId,
  pub pct_risk: f64,
  pub price_type: OrderPriceType,
  pub order_type: OrderType,
  pub direction: PositionDirection,
}

pub struct OrderPrice {
  pub price: f64,
  pub name: String,
  pub offset: f64,
}
impl OrderPrice {
  pub fn price_without_offset(&self) -> f64 {
    self.price - self.offset
  }

  pub fn price(&self) -> f64 {
    self.price
  }
}

#[derive(Clone)]
pub struct PerpOracle {
  pub market: PerpMarket,
  pub source: OracleSource,
}

#[derive(Clone)]
pub struct SpotOracle {
  pub market: SpotMarket,
  pub source: OracleSource,
}

#[derive(Clone)]
pub struct MarketInfo {
  pub price: f64,
  pub name: String,
  pub market: MarketId,
  pub spot_market: MarketId,
}

// AMM types

pub struct OptimalPegAndBudget {
  pub target_price: BigInt,
  pub new_peg: BigInt,
  pub budget: BigInt,
  pub check_lower_bound: bool,
}

pub struct AmmReservesAfterSwap {
  pub new_quote_asset_reserve: BigInt,
  pub new_base_asset_reserve: BigInt,
}

pub struct NewAmm {
  pub pre_peg_cost: BigInt,
  pub pk_numer: BigInt,
  pub pk_denom: BigInt,
  pub new_peg: BigInt,
}

pub struct SpreadReserve {
  pub base_asset_reserve: BigInt,
  pub quote_asset_reserve: BigInt,
}

pub struct OpenBidAsk {
  pub open_bids: BigInt,
  pub open_asks: BigInt,
}

pub struct Spread {
  pub long_spread: BigInt,
  pub short_spread: BigInt,
}

pub struct SpreadReserves {
  pub bid_reserves: SpreadReserve,
  pub ask_reserves: SpreadReserve,
}

pub struct VolSpread {
  pub long_vol_spread: BigInt,
  pub short_vol_spread: BigInt,
}

pub struct UpdatedAmmSpreadReserves {
  pub base_asset_reserve: BigInt,
  pub quote_asset_reserve: BigInt,
  pub sqrt_k: BigInt,
  pub new_peg: BigInt,
}

pub struct BidAsk {
  pub bid: f64,
  pub ask: f64,
}
impl BidAsk {
  pub fn spread(&self) -> f64 {
    self.ask - self.bid
  }

  pub fn mark(&self) -> f64 {
    (self.bid + self.ask) / 2.0
  }
}

pub struct MakerInfo {
  pub maker: Pubkey,
  pub maker_user_stats: Pubkey,
  pub maker_user: User,
}
