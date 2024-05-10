use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "PascalCase")]
pub enum OracleSource {
  Pyth,
  Switchboard,
  QuoteAsset,
  Pyth1K,
  Pyth1M,
  PythStableCoin,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalOracleData {
  /// precision: PRICE_PRECISION
  pub last_oracle_price: i64,
  /// precision: PRICE_PRECISION
  pub last_oracle_conf: u64,
  pub last_oracle_delay: i64,
  /// precision: PRICE_PRECISION
  pub last_oracle_price_twap: i64,
  /// precision: PRICE_PRECISION
  pub last_oracle_price_twap_5min: i64,
  pub last_oracle_price_twap_ts: i64,
}


#[derive(Serialize, Deserialize, Default, Clone, Copy, Eq, PartialEq, Debug)]
#[repr(C, packed)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalIndexData {
  /// precision: PRICE_PRECISION
  pub last_index_bid_price: u64,
  /// precision: PRICE_PRECISION
  pub last_index_ask_price: u64,
  /// precision: PRICE_PRECISION
  pub last_index_price_twap: u64,
  /// precision: PRICE_PRECISION
  pub last_index_price_twap_5min: u64,
  /// unix_timestamp of last snapshot
  pub last_index_price_twap_ts: i64,
}