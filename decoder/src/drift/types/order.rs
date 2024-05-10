use bytemuck::{CheckedBitPattern, NoUninit};
use serde::Serialize;

/// ```typescript
/// export type Order = {
///   status: OrderStatus;
///   orderType: OrderType;
///   marketType: MarketType;
///   slot: BN;
///   orderId: number;
///   userOrderId: number;
///   marketIndex: number;
///   price: BN;
///   baseAssetAmount: BN;
///   quoteAssetAmount: BN;
///   baseAssetAmountFilled: BN;
///   quoteAssetAmountFilled: BN;
///   direction: PositionDirection;
///   reduceOnly: boolean;
///   triggerPrice: BN;
///   triggerCondition: OrderTriggerCondition;
///   existingPositionDirection: PositionDirection;
///   postOnly: boolean;
///   immediateOrCancel: boolean;
///   oraclePriceOffset: number;
///   auctionDuration: number;
///   auctionStartPrice: BN;
///   auctionEndPrice: BN;
///   maxTs: BN;
/// };
/// ```
#[derive(Debug, Copy, Clone, CheckedBitPattern, NoUninit, Serialize)]
#[repr(C, packed)]
#[serde(rename_all = "camelCase")]
pub struct Order {
  pub slot: u64,
  pub price: u64,
  pub base_asset_amount: u64,
  pub base_asset_amount_filled: u64,
  pub quote_asset_amount_filled: u64,
  pub trigger_price: u64,
  pub auction_start_price: i64,
  pub auction_end_price: i64,
  pub max_ts: i64,
  pub oracle_price_offset: i32,
  pub order_id: u32,
  pub market_index: u16,
  pub status: OrderStatus,
  pub order_type: OrderType,
  pub market_type: MarketType,
  pub user_order_id: u8,
  pub existing_position_direction: PositionDirection,
  pub direction: PositionDirection,
  pub reduce_only: bool,
  pub post_only: bool,
  pub immediate_or_cancel: bool,
  pub trigger_condition: OrderTriggerCondition,
  pub auction_duration: u8,
  #[serde(with = "serde_bytes")]
  pub padding: [u8; 3],
}

/// ```typescript
/// export class OrderStatus {
///   static readonly INIT = { init: {} };
///   static readonly OPEN = { open: {} };
/// }
/// ```
#[derive(Debug, Copy, Clone, CheckedBitPattern, NoUninit, Serialize)]
#[repr(u8)]
#[serde(rename_all = "PascalCase")]
pub enum OrderStatus {
  /// The order is not in use
  Init,
  /// Order is open
  Open,
  /// Order has been filled
  Filled,
  /// Order has been canceled
  Canceled,
}

/// ```typescript
/// export class OrderType {
///   static readonly LIMIT = { limit: {} };
///   static readonly TRIGGER_MARKET = { triggerMarket: {} };
///   static readonly TRIGGER_LIMIT = { triggerLimit: {} };
///   static readonly MARKET = { market: {} };
///   static readonly ORACLE = { oracle: {} };
/// }
/// ```
#[derive(Debug, Copy, Clone, CheckedBitPattern, NoUninit, Serialize)]
#[repr(u8)]
#[serde(rename_all = "PascalCase")]
pub enum OrderType {
  Market,
  Limit,
  TriggerMarket,
  TriggerLimit,
  Oracle,
}

/// ```typescript
/// export class MarketType {
///   static readonly SPOT = { spot: {} };
///   static readonly PERP = { perp: {} };
/// }
/// ```
#[derive(Debug, Copy, Clone, CheckedBitPattern, NoUninit, Serialize)]
#[repr(u8)]
#[serde(rename_all = "PascalCase")]
pub enum MarketType {
  Spot,
  Perp,
}

/// ```typescript
/// export class PositionDirection {
///   static readonly LONG = { long: {} };
///   static readonly SHORT = { short: {} };
/// }
/// ```
#[derive(Debug, Copy, Clone, CheckedBitPattern, NoUninit, Serialize)]
#[repr(u8)]
#[serde(rename_all = "PascalCase")]
pub enum PositionDirection {
  Long,
  Short,
}

/// ```typescript
/// export class OrderTriggerCondition {
///   static readonly ABOVE = { above: {} };
///   static readonly BELOW = { below: {} };
///   static readonly TRIGGERED_ABOVE = { triggeredAbove: {} }; // above condition has been triggered
///   static readonly TRIGGERED_BELOW = { triggeredBelow: {} }; // below condition has been triggered
/// }
/// ```
#[derive(Debug, Copy, Clone, CheckedBitPattern, NoUninit, Serialize)]
#[repr(u8)]
#[serde(rename_all = "PascalCase")]
pub enum OrderTriggerCondition {
  Above,
  Below,
  TriggeredAbove, // above condition has been triggered
  TriggeredBelow, // below condition has been triggered
}