use bytemuck::{CheckedBitPattern, NoUninit};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

use common::serde::deserialize_pubkey;

use crate::drift::oracle::{HistoricalOracleData, OracleSource};

/// [Source](https://github.com/drift-labs/protocol-v2/blob/37b882d6c2be372f27b715d0f2bed5665717112f/programs/drift/src/state/user.rs#L701)
#[derive(Debug, Copy, Clone, CheckedBitPattern, NoUninit, Serialize)]
#[repr(C, packed)]
#[serde(rename_all = "camelCase")]
pub struct PerpPosition {
  pub base_asset_amount: i64,
  pub last_cumulative_funding_rate: i64,
  pub market_index: u16,
  pub quote_asset_amount: i64,
  pub quote_entry_amount: i64,
  pub quote_break_even_amount: i64,
  pub open_orders: u8,
  pub open_bids: i64,
  pub open_asks: i64,
  pub settled_pnl: i64,
  pub lp_shares: u64,
  pub remainder_base_asset_amount: i32,
  pub last_base_asset_amount_per_lp: i64,
  pub last_quote_asset_amount_per_lp: i64,
  pub per_lp_base: i8,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PerpMarket {
  #[serde(deserialize_with = "deserialize_pubkey")]
  /// The perp market's address. It is a pda of the market index
  pub pubkey: Pubkey,
  /// The automated market maker
  pub amm: AMM,
  /// The market's pnl pool. When users settle negative pnl, the balance increases.
  /// When users settle positive pnl, the balance decreases. Can not go negative.
  pub pnl_pool: PoolBalance,
  /// Encoded display name for the perp market e.g. SOL-PERP
  #[serde(with = "serde_bytes")]
  pub name: [u8; 32],
  /// The perp market's claim on the insurance fund
  pub insurance_claim: InsuranceClaim,
  /// The max pnl imbalance before positive pnl asset weight is discounted
  /// pnl imbalance is the difference between long and short pnl. When it's greater than 0,
  /// the amm has negative pnl and the initial asset weight for positive pnl is discounted
  /// precision = QUOTE_PRECISION
  pub unrealized_pnl_max_imbalance: u64,
  /// The ts when the market will be expired. Only set if market is in reduce only mode
  pub expiry_ts: i64,
  /// The price at which positions will be settled. Only set if market is expired
  /// precision = PRICE_PRECISION
  pub expiry_price: i64,
  /// Every trade has a fill record id. This is the next id to be used
  pub next_fill_record_id: u64,
  /// Every funding rate update has a record id. This is the next id to be used
  pub next_funding_rate_record_id: u64,
  /// Every amm k updated has a record id. This is the next id to be used
  pub next_curve_record_id: u64,
  /// The initial margin fraction factor. Used to increase margin ratio for large positions
  /// precision: MARGIN_PRECISION
  pub imf_factor: u32,
  /// The imf factor for unrealized pnl. Used to discount asset weight for large positive pnl
  /// precision: MARGIN_PRECISION
  pub unrealized_pnl_imf_factor: u32,
  /// The fee the liquidator is paid for taking over perp position
  /// precision: LIQUIDATOR_FEE_PRECISION
  pub liquidator_fee: u32,
  /// The fee the insurance fund receives from liquidation
  /// precision: LIQUIDATOR_FEE_PRECISION
  pub if_liquidation_fee: u32,
  /// The margin ratio which determines how much collateral is required to open a position
  /// e.g. margin ratio of .1 means a user must have $100 of total collateral to open a $1000 position
  /// precision: MARGIN_PRECISION
  pub margin_ratio_initial: u32,
  /// The margin ratio which determines when a user will be liquidated
  /// e.g. margin ratio of .05 means a user must have $50 of total collateral to maintain a $1000 position
  /// else they will be liquidated
  /// precision: MARGIN_PRECISION
  pub margin_ratio_maintenance: u32,
  /// The initial asset weight for positive pnl. Negative pnl always has an asset weight of 1
  /// precision: SPOT_WEIGHT_PRECISION
  pub unrealized_pnl_initial_asset_weight: u32,
  /// The maintenance asset weight for positive pnl. Negative pnl always has an asset weight of 1
  /// precision: SPOT_WEIGHT_PRECISION
  pub unrealized_pnl_maintenance_asset_weight: u32,
  /// number of users in a position (base)
  pub number_of_users_with_base: u32,
  /// number of users in a position (pnl) or pnl (quote)
  pub number_of_users: u32,
  pub market_index: u16,
  /// Whether a market is active, reduce only, expired, etc
  /// Affects whether users can open/close positions
  pub status: MarketStatus,
  /// Currently only Perpetual markets are supported
  pub contract_type: ContractType,
  /// The contract tier determines how much insurance a market can receive, with more speculative markets receiving less insurance
  /// It also influences the order perp markets can be liquidated, with less speculative markets being liquidated first
  pub contract_tier: ContractTier,
  pub paused_operations: u8,
  /// The spot market that pnl is settled in
  pub quote_spot_market_index: u16,
  /// Between -100 and 100, represents what % to increase/decrease the fee by
  /// E.g. if this is -50 and the fee is 5bps, the new fee will be 2.5bps
  /// if this is 50 and the fee is 5bps, the new fee will be 7.5bps
  pub fee_adjustment: i16,
  #[serde(with = "serde_bytes")]
  pub padding: [u8; 46],
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "PascalCase")]
pub enum MarketStatus {
  /// warm up period for initialization, fills are paused
  Initialized,
  /// all operations allowed
  Active,
  /// Deprecated in favor of PausedOperations
  FundingPaused,
  /// Deprecated in favor of PausedOperations
  AmmPaused,
  /// Deprecated in favor of PausedOperations
  FillPaused,
  /// Deprecated in favor of PausedOperations
  WithdrawPaused,
  /// fills only able to reduce liability
  ReduceOnly,
  /// market has determined settlement price and positions are expired must be settled
  Settlement,
  /// market has no remaining participants
  Delisted,
}

/// ```typescript
/// export class ContractType {
///   static readonly PERPETUAL = { perpetual: {} };
///   static readonly FUTURE = { future: {} };
/// }
/// ```
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub enum ContractType {
  Perpetual,
  Future,
}

/// ```typescript
/// export class ContractTier {
///   static readonly A = { a: {} };
///   static readonly B = { b: {} };
///   static readonly C = { c: {} };
///   static readonly SPECULATIVE = { speculative: {} };
///   static readonly ISOLATED = { isolated: {} };
/// }
/// ```
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub enum ContractTier {
  /// max insurance capped at A level
  A,
  /// max insurance capped at B level
  B,
  /// max insurance capped at C level
  C,
  /// no insurance
  Speculative,
  /// no insurance, only single position allowed
  Isolated,
}

/// ```typescript
/// export type PoolBalance = {
///   scaledBalance: BN;
///   marketIndex: number;
/// };
/// ```
#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct PoolBalance {
  /// To get the pool's token amount, you must multiply the scaled balance by the market's cumulative
  /// deposit interest
  /// precision: SPOT_BALANCE_PRECISION
  pub scaled_balance: u128,
  /// The spot market the pool is for
  pub market_index: u16,
  #[serde(with = "serde_bytes")]
  pub padding: [u8; 6],
}

/// ```typescript
/// export type InsuranceClaim = {
///   revenueWithdrawSinceLastSettle: BN;
///   maxRevenueWithdrawPerPeriod: BN;
///   lastRevenueWithdrawTs: BN;
///   quoteSettledInsurance: BN;
///   quoteMaxInsurance: BN;
/// };
/// ```
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InsuranceClaim {
  /// The amount of revenue last settled
  /// Positive if funds left the perp market,
  /// negative if funds were pulled into the perp market
  /// precision: QUOTE_PRECISION
  pub revenue_withdraw_since_last_settle: i64,
  /// The max amount of revenue that can be withdrawn per period
  /// precision: QUOTE_PRECISION
  pub max_revenue_withdraw_per_period: u64,
  /// The max amount of insurance that perp market can use to resolve bankruptcy and pnl deficits
  /// precision: QUOTE_PRECISION
  pub quote_max_insurance: u64,
  /// The amount of insurance that has been used to resolve bankruptcy and pnl deficits
  /// precision: QUOTE_PRECISION
  pub quote_settled_insurance: u64,
  /// The last time revenue was settled in/out of market
  pub last_revenue_withdraw_ts: i64,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AMM {
  /// oracle price data public key
  #[serde(deserialize_with = "deserialize_pubkey")]
  pub oracle: Pubkey,
  /// stores historically witnessed oracle data
  pub historical_oracle_data: HistoricalOracleData,
  /// accumulated base asset amount since inception per lp share
  pub base_asset_amount_per_lp: i128,
  /// accumulated quote asset amount since inception per lp share
  pub quote_asset_amount_per_lp: i128,
  /// partition of fees from perp market trading moved from pnl settlements
  pub fee_pool: PoolBalance,
  /// `x` reserves for constant product mm formula (x * y = k)
  pub base_asset_reserve: u128,
  /// `y` reserves for constant product mm formula (x * y = k)
  pub quote_asset_reserve: u128,
  /// determines how close the min/max base asset reserve sit vs base reserves
  /// allow for decreasing slippage without increasing liquidity and v.v.
  pub concentration_coef: u128,
  /// minimum base_asset_reserve allowed before AMM is unavailable
  pub min_base_asset_reserve: u128,
  /// maximum base_asset_reserve allowed before AMM is unavailable
  pub max_base_asset_reserve: u128,
  /// `sqrt(k)` in constant product mm formula (x * y = k). stored to avoid drift caused by integer math issues
  pub sqrt_k: u128,
  /// normalizing numerical factor for y, its use offers lowest slippage in cp-curve when market is balanced
  pub peg_multiplier: u128,
  /// y when market is balanced. stored to save computation
  pub terminal_quote_asset_reserve: u128,
  /// tracks number of total longs in market (regardless of counterparty)
  pub base_asset_amount_long: i128,
  /// tracks number of total shorts in market (regardless of counterparty)
  pub base_asset_amount_short: i128,
  /// tracks net position (longs-shorts) in market with AMM as counterparty
  pub base_asset_amount_with_amm: i128,
  /// tracks net position (longs-shorts) in market with LPs as counterparty
  pub base_asset_amount_with_unsettled_lp: i128,
  /// max allowed open interest, blocks trades that breach this value
  pub max_open_interest: u128,
  /// sum of all user's perp quote_asset_amount in market
  pub quote_asset_amount: i128,
  /// sum of all long user's quote_entry_amount in market
  pub quote_entry_amount_long: i128,
  /// sum of all short user's quote_entry_amount in market
  pub quote_entry_amount_short: i128,
  /// sum of all long user's quote_break_even_amount in market
  pub quote_break_even_amount_long: i128,
  /// sum of all short user's quote_break_even_amount in market
  pub quote_break_even_amount_short: i128,
  /// total user lp shares of sqrt_k (protocol owned liquidity = sqrt_k - last_funding_rate)
  pub user_lp_shares: u128,
  /// last funding rate in this perp market (unit is quote per base)
  pub last_funding_rate: i64,
  /// last funding rate for longs in this perp market (unit is quote per base)
  pub last_funding_rate_long: i64,
  /// last funding rate for shorts in this perp market (unit is quote per base)
  pub last_funding_rate_short: i64,
  /// estimate of last 24h of funding rate perp market (unit is quote per base)
  pub last_24h_avg_funding_rate: i64,
  /// total fees collected by this perp market
  pub total_fee: i128,
  /// total fees collected by the vAMM's bid/ask spread
  pub total_mm_fee: i128,
  /// total fees collected by exchange fee schedule
  pub total_exchange_fee: u128,
  /// total fees minus any recognized upnl and pool withdraws
  pub total_fee_minus_distributions: i128,
  /// sum of all fees from fee pool withdrawn to revenue pool
  pub total_fee_withdrawn: u128,
  /// all fees collected by market for liquidations
  pub total_liquidation_fee: u128,
  /// accumulated funding rate for longs since inception in market
  pub cumulative_funding_rate_long: i128,
  /// accumulated funding rate for shorts since inception in market
  pub cumulative_funding_rate_short: i128,
  /// accumulated social loss paid by users since inception in market
  pub total_social_loss: u128,
  /// transformed base_asset_reserve for users going long
  pub ask_base_asset_reserve: u128,
  /// transformed quote_asset_reserve for users going long
  pub ask_quote_asset_reserve: u128,
  /// transformed base_asset_reserve for users going short
  pub bid_base_asset_reserve: u128,
  /// transformed quote_asset_reserve for users going short
  pub bid_quote_asset_reserve: u128,
  /// the last seen oracle price partially shrunk toward the amm reserve price
  /// precision: PRICE_PRECISION
  pub last_oracle_normalised_price: i64,
  /// the gap between the oracle price and the reserve price = y * peg_multiplier / x
  pub last_oracle_reserve_price_spread_pct: i64,
  /// average estimate of bid price over funding_period
  /// precision: PRICE_PRECISION
  pub last_bid_price_twap: u64,
  /// average estimate of ask price over funding_period
  /// precision: PRICE_PRECISION
  pub last_ask_price_twap: u64,
  /// average estimate of (bid+ask)/2 price over funding_period
  /// precision: PRICE_PRECISION
  pub last_mark_price_twap: u64,
  /// average estimate of (bid+ask)/2 price over FIVE_MINUTES
  pub last_mark_price_twap_5min: u64,
  /// the last blockchain slot the amm was updated
  pub last_update_slot: u64,
  /// the pct size of the oracle confidence interval
  /// precision: PERCENTAGE_PRECISION
  pub last_oracle_conf_pct: u64,
  /// the total_fee_minus_distribution change since the last funding update
  /// precision: QUOTE_PRECISION
  pub net_revenue_since_last_funding: i64,
  /// the last funding rate update unix_timestamp
  pub last_funding_rate_ts: i64,
  /// the peridocity of the funding rate updates
  pub funding_period: i64,
  /// the base step size (increment) of orders
  /// precision: BASE_PRECISION
  pub order_step_size: u64,
  /// the price tick size of orders
  /// precision: PRICE_PRECISION
  pub order_tick_size: u64,
  /// the minimum base size of an order
  /// precision: BASE_PRECISION
  pub min_order_size: u64,
  /// the max base size a single user can have
  /// precision: BASE_PRECISION
  pub max_position_size: u64,
  /// estimated total of volume in market
  /// QUOTE_PRECISION
  pub volume_24h: u64,
  /// the volume intensity of long fills against AMM
  pub long_intensity_volume: u64,
  /// the volume intensity of short fills against AMM
  pub short_intensity_volume: u64,
  /// the blockchain unix timestamp at the time of the last trade
  pub last_trade_ts: i64,
  /// estimate of standard deviation of the fill (mark) prices
  /// precision: PRICE_PRECISION
  pub mark_std: u64,
  /// estimate of standard deviation of the oracle price at each update
  /// precision: PRICE_PRECISION
  pub oracle_std: u64,
  /// the last unix_timestamp the mark twap was updated
  pub last_mark_price_twap_ts: i64,
  /// the minimum spread the AMM can quote. also used as step size for some spread logic increases.
  pub base_spread: u32,
  /// the maximum spread the AMM can quote
  pub max_spread: u32,
  /// the spread for asks vs the reserve price
  pub long_spread: u32,
  /// the spread for bids vs the reserve price
  pub short_spread: u32,
  /// the count intensity of long fills against AMM
  pub long_intensity_count: u32,
  /// the count intensity of short fills against AMM
  pub short_intensity_count: u32,
  /// the fraction of total available liquidity a single fill on the AMM can consume
  pub max_fill_reserve_fraction: u16,
  /// the maximum slippage a single fill on the AMM can push
  pub max_slippage_ratio: u16,
  /// the update intensity of AMM formulaic updates (adjusting k). 0-100
  pub curve_update_intensity: u8,
  /// the jit intensity of AMM. larger intensity means larger participation in jit. 0 means no jit participation.
  /// (0, 100] is intensity for protocol-owned AMM. (100, 200] is intensity for user LP-owned AMM.
  pub amm_jit_intensity: u8,
  /// the oracle provider information. used to decode/scale the oracle public key
  pub oracle_source: OracleSource,
  /// tracks whether the oracle was considered valid at the last AMM update
  pub last_oracle_valid: bool,
  /// the target value for `base_asset_amount_per_lp`, used during AMM JIT with LP split
  /// precision: BASE_PRECISION
  pub target_base_asset_amount_per_lp: i32,
  /// expo for unit of per_lp, base 10 (if per_lp_base=X, then per_lp unit is 10^X)
  pub per_lp_base: i8,
  pub padding1: u8,
  pub padding2: u16,
  pub total_fee_earned_per_lp: u64,
  pub net_unsettled_funding_pnl: i64,
  pub quote_asset_amount_with_unsettled_lp: i64,
  pub reference_price_offset: i32,
  #[serde(with = "serde_bytes")]
  pub padding: [u8; 12],
}