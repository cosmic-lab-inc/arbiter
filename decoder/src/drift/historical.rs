use common::{Data, trunc};

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

#[derive(Debug, Clone)]
pub struct HistoricalPerformance(pub Vec<HistoricalSettlePnl>);

impl HistoricalPerformance {
  pub fn user(&self) -> String {
    self.0[0].user.clone()
  }

  pub fn summary(&self) -> Vec<TradeRecord> {
    let mut cum_pnl = 0.0;
    let mut stubs = vec![];
    for record in self.0.iter() {
      cum_pnl += record.pnl;
      stubs.push(TradeRecord {
        cum_quote_pnl: trunc!(cum_pnl, 2),
        trade_quote_pnl: trunc!(record.pnl, 2),
        user: record.user.clone(),
        ts: record.ts,
      });
    }
    stubs
  }

  pub fn avg_quote_pnl(&self) -> f64 {
    let mut cum_pnl = 0.0;
    for record in self.0.iter() {
      cum_pnl += record.pnl;
    }
    cum_pnl / self.0.len() as f64
  }

  pub fn dataset(&self) -> Vec<Data> {
    self.summary().into_iter().map(|s| {
      Data {
        x: s.ts as i64,
        y: s.cum_quote_pnl,
      }
    }).collect()
  }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TradeRecord {
  pub cum_quote_pnl: f64,
  pub trade_quote_pnl: f64,
  pub user: String,
  pub ts: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PnlStub {
  pub user: String,
  pub avg_quote_pnl: f64
}