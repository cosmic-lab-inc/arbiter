use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use anchor_lang::prelude::{AccountInfo, AccountMeta};
use solana_sdk::pubkey::Pubkey;

use crate::drift_client::DriftUtils;
use drift_cpi::{
  MarketType, OraclePriceData, OracleSource, Order, OrderType, PerpMarket, PositionDirection,
  SpotMarket, User, BASE_PRECISION, PRICE_PRECISION,
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
}

impl DlobNode {
  pub fn new(order: Order) -> Self {
    Self { order }
  }

  pub fn price(&self, oracle_price: f64) -> anyhow::Result<f64> {
    Ok(match self.order.price == 0 {
      true => {
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

  pub fn is_ask(&self) -> bool {
    matches!(self.order.direction, PositionDirection::Short)
  }

  pub fn bid(&self, oracle_price: f64) -> anyhow::Result<BidAsk> {
    if !self.is_bid() {
      return Err(anyhow::anyhow!("Order is not a bid"));
    }
    Ok(BidAsk {
      price: self.price(oracle_price)?,
      size: self.size(),
      slot: self.slot(),
    })
  }

  pub fn ask(&self, oracle_price: f64) -> anyhow::Result<BidAsk> {
    if !self.is_ask() {
      return Err(anyhow::anyhow!("Order is not an ask"));
    }
    Ok(BidAsk {
      price: self.price(oracle_price)?,
      size: self.size(),
      slot: self.slot(),
    })
  }
}

pub struct BidAsk {
  pub price: f64,
  pub size: f64,
  pub slot: u64,
}

pub struct L3Orderbook {
  /// First index is highest/best bid
  pub bids: Vec<BidAsk>,
  /// First index is lowest/best ask
  pub asks: Vec<BidAsk>,
  pub spread: f64,
  pub slot: u64,
  pub oracle_price: f64,
}
impl L3Orderbook {
  pub fn best_bid(&self) -> anyhow::Result<&BidAsk> {
    self
      .bids
      .iter()
      .filter(|b| b.price < self.oracle_price)
      .max_by(|a, b| a.price.partial_cmp(&b.price).unwrap())
      .ok_or(anyhow::anyhow!("No bids"))
  }

  pub fn best_ask(&self) -> anyhow::Result<&BidAsk> {
    self
      .asks
      .iter()
      .filter(|b| b.price > self.oracle_price)
      .min_by(|a, b| a.price.partial_cmp(&b.price).unwrap())
      .ok_or(anyhow::anyhow!("No asks"))
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
