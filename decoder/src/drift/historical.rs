/// https://docs.drift.trade/historical-data/historical-data-glossary#settle-pnl
#[derive(Debug, Clone, borsh::BorshSerialize, borsh::BorshDeserialize, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalSettlePnl {
  pub pnl: f64,
  pub user: String,
  pub base_asset_amount: f64,
  pub quote_asset_amount_after: f64,
  pub quote_entry_amount_before: f64,
  pub settle_price: f64,
  pub tx_sig: String,
  pub slot: u64,
  pub ts: u64,
  pub market_index: u16,
  pub explanation: String,
  pub program_id: String
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PnlStub {
  pub pnl: f64,
  pub user: String,
  pub ts: u64,
}