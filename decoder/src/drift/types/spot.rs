use bytemuck::{CheckedBitPattern, NoUninit};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

use crate::drift::oracle::{HistoricalIndexData, HistoricalOracleData, OracleSource};
use crate::perp::{MarketStatus, PoolBalance};

#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Debug, Eq, PartialOrd, Ord)]
#[repr(u8)]
#[serde(rename_all = "PascalCase")]
pub enum AssetTier {
  /// full priviledge
  Collateral,
  /// collateral, but no borrow
  Protected,
  /// not collateral, allow multi-borrow
  Cross,
  /// not collateral, only single borrow
  Isolated,
  /// no privilege
  Unlisted,
}

#[derive(Serialize, Deserialize, Default, Eq, PartialEq, Debug, Copy, Clone)]
#[repr(C)]
#[serde(rename_all = "camelCase")]
pub struct InsuranceFund {
  pub vault: Pubkey,
  pub total_shares: u128,
  pub user_shares: u128,
  pub shares_base: u128,     // exponent for lp shares (for rebasing)
  pub unstaking_period: i64, // if_unstaking_period
  pub last_revenue_settle_ts: i64,
  pub revenue_settle_period: i64,
  pub total_factor: u32, // percentage of interest for total insurance
  pub user_factor: u32,  // percentage of interest for user staked insurance
}

#[derive(Debug, Clone, Copy, CheckedBitPattern, NoUninit, Serialize)]
#[repr(u8)]
#[serde(rename_all = "PascalCase")]
pub enum SpotBalanceType {
  Deposit,
  Borrow,
}

#[derive(Debug, Copy, Clone, CheckedBitPattern, NoUninit, Serialize)]
#[repr(C, packed)]
#[serde(rename_all = "camelCase")]
pub struct SpotPosition {
  pub scaled_balance: u64,
  pub open_bids: i64,
  pub open_asks: i64,
  pub cumulative_deposits: i64,
  pub market_index: u16,
  pub balance_type: SpotBalanceType,
  pub open_orders: u8,
  #[serde(with = "serde_bytes")]
  pub padding: [u8; 4],
}

#[derive(Deserialize, Debug)]
#[repr(C, packed)]
#[serde(rename_all = "camelCase")]
pub struct SpotMarket {
  #[serde(serialize_with = "serialize_pubkey")]
  pub pubkey: Pubkey,
  #[serde(serialize_with = "serialize_pubkey")]
  pub oracle: Pubkey,
  #[serde(serialize_with = "serialize_pubkey")]
  pub mint: Pubkey,
  #[serde(serialize_with = "serialize_pubkey")]
  pub vault: Pubkey,
  #[serde(with = "serde_bytes")]
  pub name: [u8; 32],
  pub historical_oracle_data: HistoricalOracleData,
  pub historical_index_data: HistoricalIndexData,
  pub revenue_pool: PoolBalance, // in base asset
  pub spot_fee_pool: PoolBalance,
  pub insurance_fund: InsuranceFund,
  pub total_spot_fee: u128,
  pub deposit_balance: u128,
  pub borrow_balance: u128,
  pub cumulative_deposit_interest: u128,
  pub cumulative_borrow_interest: u128,
  pub total_social_loss: u128,
  pub total_quote_social_loss: u128,
  pub withdraw_guard_threshold: u64,
  pub max_token_deposits: u64,
  pub deposit_token_twap: u64,
  pub borrow_token_twap: u64,
  pub utilization_twap: u64,
  pub last_interest_ts: u64,
  pub last_twap_ts: u64,
  pub expiry_ts: i64,
  pub order_step_size: u64,
  pub order_tick_size: u64,
  pub min_order_size: u64,
  pub max_position_size: u64,
  pub next_fill_record_id: u64,
  pub next_deposit_record_id: u64,
  pub initial_asset_weight: u32,
  pub maintenance_asset_weight: u32,
  pub initial_liability_weight: u32,
  pub maintenance_liability_weight: u32,
  pub imf_factor: u32,
  pub liquidator_fee: u32,
  pub if_liquidation_fee: u32,
  pub optimal_utilization: u32,
  pub optimal_borrow_rate: u32,
  pub max_borrow_rate: u32,
  pub decimals: u32,
  pub market_index: u16,
  pub orders_enabled: bool,
  pub oracle_source: OracleSource,
  pub status: MarketStatus,
  pub asset_tier: AssetTier,
  pub paused_operations: u8,
  #[serde(with = "serde_bytes")]
  pub padding1: [u8; 5],
  pub flash_loan_amount: u64,
  pub flash_loan_initial_token_amount: u64,
  pub total_swap_fee: u64,
  pub scale_initial_asset_weight_start: u64,
  #[serde(with = "serde_bytes")]
  pub padding: [u8; 48],
}