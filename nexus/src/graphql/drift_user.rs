use crate::{deserialize_i64, deserialize_pubkey, deserialize_u64};
use drift_cpi::{
  MarketType, Order, OrderStatus, OrderTriggerCondition, OrderType, PerpPosition,
  PositionDirection, SpotBalanceType, SpotPosition, User,
};
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct _User {
  #[serde(deserialize_with = "deserialize_pubkey")]
  pub authority: Pubkey,
  #[serde(deserialize_with = "deserialize_pubkey")]
  pub delegate: Pubkey,
  pub name: [u8; 32],
  pub spot_positions: [_SpotPosition; 8],
  pub perp_positions: [_PerpPosition; 8],
  pub orders: [_Order; 32],
  pub last_add_perp_lp_shares_ts: i64,
  pub total_deposits: u64,
  pub total_withdraws: u64,
  pub total_social_loss: u64,
  pub settled_perp_pnl: i64,
  pub cumulative_spot_fees: i64,
  pub cumulative_perp_funding: i64,
  pub liquidation_margin_freed: u64,
  pub last_active_slot: u64,
  pub next_order_id: u32,
  pub max_margin_ratio: u32,
  pub next_liquidation_id: u16,
  pub sub_account_id: u16,
  pub status: u8,
  pub is_margin_trading_enabled: bool,
  pub idle: bool,
  pub open_orders: u8,
  pub has_open_order: bool,
  pub open_auctions: u8,
  pub has_open_auction: bool,
  pub padding: [u8; 21],
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct _SpotPosition {
  #[serde(deserialize_with = "deserialize_u64")]
  pub scaled_balance: u64,
  #[serde(deserialize_with = "deserialize_i64")]
  pub open_bids: i64,
  #[serde(deserialize_with = "deserialize_i64")]
  pub open_asks: i64,
  #[serde(deserialize_with = "deserialize_i64")]
  pub cumulative_deposits: i64,
  pub market_index: u16,
  pub balance_type: _SpotBalanceType,
  pub open_orders: u8,
  pub padding: [u8; 4],
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct _PerpPosition {
  #[serde(deserialize_with = "deserialize_i64")]
  pub last_cumulative_funding_rate: i64,
  #[serde(deserialize_with = "deserialize_i64")]
  pub base_asset_amount: i64,
  #[serde(deserialize_with = "deserialize_i64")]
  pub quote_asset_amount: i64,
  #[serde(deserialize_with = "deserialize_i64")]
  pub quote_break_even_amount: i64,
  #[serde(deserialize_with = "deserialize_i64")]
  pub quote_entry_amount: i64,
  #[serde(deserialize_with = "deserialize_i64")]
  pub open_bids: i64,
  #[serde(deserialize_with = "deserialize_i64")]
  pub open_asks: i64,
  #[serde(deserialize_with = "deserialize_i64")]
  pub settled_pnl: i64,
  #[serde(deserialize_with = "deserialize_u64")]
  pub lp_shares: u64,
  #[serde(deserialize_with = "deserialize_i64")]
  pub last_base_asset_amount_per_lp: i64,
  #[serde(deserialize_with = "deserialize_i64")]
  pub last_quote_asset_amount_per_lp: i64,
  pub remainder_base_asset_amount: i32,
  pub market_index: u16,
  pub open_orders: u8,
  pub per_lp_base: i8,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct _Order {
  #[serde(deserialize_with = "deserialize_u64")]
  pub slot: u64,
  #[serde(deserialize_with = "deserialize_u64")]
  pub price: u64,
  #[serde(deserialize_with = "deserialize_u64")]
  pub base_asset_amount: u64,
  #[serde(deserialize_with = "deserialize_u64")]
  pub base_asset_amount_filled: u64,
  #[serde(deserialize_with = "deserialize_u64")]
  pub quote_asset_amount_filled: u64,
  #[serde(deserialize_with = "deserialize_u64")]
  pub trigger_price: u64,
  #[serde(deserialize_with = "deserialize_i64")]
  pub auction_start_price: i64,
  #[serde(deserialize_with = "deserialize_i64")]
  pub auction_end_price: i64,
  #[serde(deserialize_with = "deserialize_i64")]
  pub max_ts: i64,
  pub oracle_price_offset: i32,
  pub order_id: u32,
  pub market_index: u16,
  pub status: _OrderStatus,
  pub order_type: _OrderType,
  pub market_type: _MarketType,
  pub user_order_id: u8,
  pub existing_position_direction: _PositionDirection,
  pub direction: _PositionDirection,
  pub reduce_only: bool,
  pub post_only: bool,
  pub immediate_or_cancel: bool,
  pub trigger_condition: _OrderTriggerCondition,
  pub auction_duration: u8,
  pub padding: [u8; 3],
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EmptyStruct {}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum _SpotBalanceType {
  Deposit {},
  Borrow {},
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum _OrderStatus {
  Init {},
  Open {},
  Filled {},
  Canceled {},
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum _OrderTriggerCondition {
  Above {},
  Below {},
  TriggeredAbove {},
  TriggeredBelow {},
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum _MarketType {
  Spot {},
  Perp {},
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum _OrderType {
  Market {},
  Limit {},
  TriggerMarket {},
  TriggerLimit {},
  Oracle {},
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum _PositionDirection {
  Long {},
  Short {},
}
// impl<'de> Deserialize<'de> for _PositionDirection {
//   fn deserialize<D>(deserializer: D) -> Result<_PositionDirection, D::Error>
//   where
//     D: serde::Deserializer<'de>,
//   {
//     struct _PositionDirectionVisitor;
//     impl<'de> serde::de::Visitor<'de> for _PositionDirectionVisitor {
//       type Value = _PositionDirection;
//       fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
//         formatter.write_str("enum _PositionDirection")
//       }
//       fn visit_map<A>(self, mut map: A) -> Result<_PositionDirection, A::Error>
//       where
//         A: serde::de::MapAccess<'de>,
//       {
//         let key: String = map
//           .next_key()
//           .unwrap()
//           .ok_or(serde::de::Error::custom("missing key"))?;
//         match key.as_str() {
//           "long" => Ok(_PositionDirection::Long),
//           "short" => Ok(_PositionDirection::Short),
//           _ => Err(serde::de::Error::custom(format!("unknown key: {}", key))),
//         }
//       }
//     }
//     deserializer.deserialize_map(_PositionDirectionVisitor)
//   }
// }

impl TryInto<User> for _User {
  type Error = anyhow::Error;
  fn try_into(self) -> anyhow::Result<User> {
    Ok(User {
      authority: self.authority,
      delegate: self.delegate,
      name: self.name,
      spot_positions: self
        .spot_positions
        .iter()
        .map(|x| x.clone().into())
        .collect::<Vec<SpotPosition>>()[..]
        .try_into()?,
      perp_positions: self
        .perp_positions
        .iter()
        .map(|x| x.clone().into())
        .collect::<Vec<PerpPosition>>()[..]
        .try_into()?,
      orders: self
        .orders
        .iter()
        .map(|x| x.clone().into())
        .collect::<Vec<Order>>()[..]
        .try_into()?,
      last_add_perp_lp_shares_ts: self.last_add_perp_lp_shares_ts,
      total_deposits: self.total_deposits,
      total_withdraws: self.total_withdraws,
      total_social_loss: self.total_social_loss,
      settled_perp_pnl: self.settled_perp_pnl,
      cumulative_spot_fees: self.cumulative_spot_fees,
      cumulative_perp_funding: self.cumulative_perp_funding,
      liquidation_margin_freed: self.liquidation_margin_freed,
      last_active_slot: self.last_active_slot,
      next_order_id: self.next_order_id,
      max_margin_ratio: self.max_margin_ratio,
      next_liquidation_id: self.next_liquidation_id,
      sub_account_id: self.sub_account_id,
      status: self.status,
      is_margin_trading_enabled: self.is_margin_trading_enabled,
      idle: self.idle,
      open_orders: self.open_orders,
      has_open_order: self.has_open_order,
      open_auctions: self.open_auctions,
      has_open_auction: self.has_open_auction,
      padding: self.padding,
    })
  }
}

impl Into<SpotPosition> for _SpotPosition {
  fn into(self) -> SpotPosition {
    SpotPosition {
      scaled_balance: self.scaled_balance,
      open_bids: self.open_bids,
      open_asks: self.open_asks,
      cumulative_deposits: self.cumulative_deposits,
      market_index: self.market_index,
      balance_type: self.balance_type.into(),
      open_orders: self.open_orders,
      padding: self.padding,
    }
  }
}

impl Into<SpotBalanceType> for _SpotBalanceType {
  fn into(self) -> SpotBalanceType {
    match self {
      _SpotBalanceType::Deposit { .. } => SpotBalanceType::Deposit,
      _SpotBalanceType::Borrow { .. } => SpotBalanceType::Borrow,
    }
  }
}

impl Into<PerpPosition> for _PerpPosition {
  fn into(self) -> PerpPosition {
    PerpPosition {
      last_cumulative_funding_rate: self.last_cumulative_funding_rate,
      base_asset_amount: self.base_asset_amount,
      quote_asset_amount: self.quote_asset_amount,
      quote_break_even_amount: self.quote_break_even_amount,
      quote_entry_amount: self.quote_entry_amount,
      open_bids: self.open_bids,
      open_asks: self.open_asks,
      settled_pnl: self.settled_pnl,
      lp_shares: self.lp_shares,
      last_base_asset_amount_per_lp: self.last_base_asset_amount_per_lp,
      last_quote_asset_amount_per_lp: self.last_quote_asset_amount_per_lp,
      remainder_base_asset_amount: self.remainder_base_asset_amount,
      market_index: self.market_index,
      open_orders: self.open_orders,
      per_lp_base: self.per_lp_base,
    }
  }
}

impl Into<Order> for _Order {
  fn into(self) -> Order {
    Order {
      slot: self.slot,
      price: self.price,
      base_asset_amount: self.base_asset_amount,
      base_asset_amount_filled: self.base_asset_amount_filled,
      quote_asset_amount_filled: self.quote_asset_amount_filled,
      trigger_price: self.trigger_price,
      auction_start_price: self.auction_start_price,
      auction_end_price: self.auction_end_price,
      max_ts: self.max_ts,
      oracle_price_offset: self.oracle_price_offset,
      order_id: self.order_id,
      market_index: self.market_index,
      status: self.status.into(),
      order_type: self.order_type.into(),
      market_type: self.market_type.into(),
      user_order_id: self.user_order_id,
      existing_position_direction: self.existing_position_direction.into(),
      direction: self.direction.into(),
      reduce_only: self.reduce_only,
      post_only: self.post_only,
      immediate_or_cancel: self.immediate_or_cancel,
      trigger_condition: self.trigger_condition.into(),
      auction_duration: self.auction_duration,
      padding: self.padding,
    }
  }
}

impl Into<PositionDirection> for _PositionDirection {
  fn into(self) -> PositionDirection {
    match self {
      _PositionDirection::Long { .. } => PositionDirection::Long,
      _PositionDirection::Short { .. } => PositionDirection::Short,
    }
  }
}

impl Into<OrderTriggerCondition> for _OrderTriggerCondition {
  fn into(self) -> OrderTriggerCondition {
    match self {
      _OrderTriggerCondition::Above { .. } => OrderTriggerCondition::Above,
      _OrderTriggerCondition::Below { .. } => OrderTriggerCondition::Below,
      _OrderTriggerCondition::TriggeredAbove { .. } => OrderTriggerCondition::TriggeredAbove,
      _OrderTriggerCondition::TriggeredBelow { .. } => OrderTriggerCondition::TriggeredBelow,
    }
  }
}

impl Into<MarketType> for _MarketType {
  fn into(self) -> MarketType {
    match self {
      _MarketType::Spot { .. } => MarketType::Spot,
      _MarketType::Perp { .. } => MarketType::Perp,
    }
  }
}

impl Into<OrderStatus> for _OrderStatus {
  fn into(self) -> OrderStatus {
    match self {
      _OrderStatus::Init { .. } => OrderStatus::Init,
      _OrderStatus::Open { .. } => OrderStatus::Open,
      _OrderStatus::Filled { .. } => OrderStatus::Filled,
      _OrderStatus::Canceled { .. } => OrderStatus::Canceled,
    }
  }
}

impl Into<OrderType> for _OrderType {
  fn into(self) -> OrderType {
    match self {
      _OrderType::Market { .. } => OrderType::Market,
      _OrderType::Limit { .. } => OrderType::Limit,
      _OrderType::TriggerMarket { .. } => OrderType::TriggerMarket,
      _OrderType::TriggerLimit { .. } => OrderType::TriggerLimit,
      _OrderType::Oracle { .. } => OrderType::Oracle,
    }
  }
}
