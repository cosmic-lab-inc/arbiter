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

pub struct HistoricalPerformance(pub Vec<HistoricalSettlePnl>);

impl HistoricalPerformance {
  pub fn summary(&self) -> Vec<PnlStub> {
    let mut cum_pnl = 0.0;
    let mut stubs = vec![];
    for record in self.0.iter() {
      cum_pnl += record.pnl;
      stubs.push(PnlStub {
        cum_quote_pnl: trunc!(cum_pnl, 2),
        trade_quote_pnl: trunc!(record.pnl, 2),
        user: record.user.clone(),
        ts: record.ts,
      });
    }
    stubs
  }

  pub fn dataset(&self) -> Vec<Data> {
    let data = self.summary().into_iter().map(|s| {
      Data {
        x: s.ts as i64,
        y: s.cum_quote_pnl,
      }
    }).collect();
    data
  }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PnlStub {
  pub cum_quote_pnl: f64,
  pub trade_quote_pnl: f64,
  pub user: String,
  pub ts: u64,
}
