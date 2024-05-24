use solana_sdk::clock::Slot;
use solana_sdk::pubkey::Pubkey;

use drift_cpi::{QUOTE_PRECISION, User, UserStats};

use crate::{DecodedAcctCtx, trunc};

pub struct TokenBalance {
  pub balance: u128,
  pub mint: Pubkey,
}

pub struct DriftTrader {
  pub authority: Pubkey,
  pub user_stats: DecodedAcctCtx<UserStats>,
  pub users: Vec<DecodedAcctCtx<User>>,
}

impl DriftTrader {
  pub fn settled_perp_pnl(&self) -> f64 {
    // iterate each UserAccountInfo.account.settled_perp_pnl and sum
    let sum: f64 = self
      .users
      .iter()
      .map(|u| (u.decoded.settled_perp_pnl as f64) / (QUOTE_PRECISION as f64))
      .sum();
    trunc!(sum, 3)
  }
  pub fn total_deposits(&self) -> f64 {
    // iterate each UserAccountInfo.account.total_deposits and sum
    let sum: f64 = self
      .users
      .iter()
      .map(|u| (u.decoded.total_deposits as f64) / (QUOTE_PRECISION as f64))
      .sum();
    trunc!(sum, 3)
  }
  pub fn taker_volume_30d(&self) -> f64 {
    trunc!(
        self.user_stats.decoded.taker_volume30d as f64 / (QUOTE_PRECISION as f64),
        3
    )
  }
  pub fn maker_volume_30d(&self) -> f64 {
    trunc!(
        self.user_stats.decoded.maker_volume30d as f64 / (QUOTE_PRECISION as f64),
        3
    )
  }
  pub fn roi(&self) -> Option<f64> {
    match self.total_deposits() > 0_f64 {
      true => Some(trunc!(self.settled_perp_pnl() / self.total_deposits(), 3)),
      false => None,
    }
  }
  pub fn pnl_per_volume(&self) -> f64 {
    trunc!(
        self.settled_perp_pnl() / (self.taker_volume_30d() + self.maker_volume_30d()),
        3
    )
  }
  pub fn best_user(&self) -> &DecodedAcctCtx<User> {
    self.users
        .iter()
        .max_by_key(|u| u.decoded.settled_perp_pnl)
        .unwrap()
  }
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct TraderStub {
  pub authority: String,
  pub pnl: f64,
  pub best_user: String
}

pub struct DriftTraderSnapshot {
  pub slot: u64,
  pub trader: DriftTrader,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
pub struct TraderStats {
  pub authority: Pubkey,
  pub best_user: Pubkey,
  pub settled_perp_pnl: f64,
  pub total_deposits: f64,
  pub roi: Option<f64>,
  pub taker_volume_30d: f64,
  pub maker_volume_30d: f64,
  pub pnl_per_volume: f64,
  pub slot: Option<Slot>,
}

impl From<DriftTrader> for TraderStats {
  fn from(trader: DriftTrader) -> Self {
    TraderStats {
      authority: trader.authority,
      best_user: trader.best_user().key,
      settled_perp_pnl: trader.settled_perp_pnl(),
      total_deposits: trader.total_deposits(),
      roi: trader.roi(),
      taker_volume_30d: trader.taker_volume_30d(),
      maker_volume_30d: trader.maker_volume_30d(),
      pnl_per_volume: trader.pnl_per_volume(),
      slot: None,
    }
  }
}

impl From<DriftTraderSnapshot> for TraderStats {
  fn from(snapshot: DriftTraderSnapshot) -> Self {
    TraderStats {
      authority: snapshot.trader.authority,
      best_user: snapshot.trader.best_user().key,
      settled_perp_pnl: snapshot.trader.settled_perp_pnl(),
      total_deposits: snapshot.trader.total_deposits(),
      roi: snapshot.trader.roi(),
      taker_volume_30d: snapshot.trader.taker_volume_30d(),
      maker_volume_30d: snapshot.trader.maker_volume_30d(),
      pnl_per_volume: snapshot.trader.pnl_per_volume(),
      slot: Some(snapshot.slot),
    }
  }
}

impl std::fmt::Display for TraderStats {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "TraderStats: authority: {}, settled_perp_pnl: {}, total_deposits: {}, roi: {:?}, taker_volume_30d: {}, \
            maker_volume_30d: {}, pnl_per_volume: {}, slot: {}",
      self.authority,
      self.settled_perp_pnl,
      self.total_deposits,
      self.roi,
      self.taker_volume_30d,
      self.maker_volume_30d,
      self.pnl_per_volume,
      match self.slot {
        Some(slot) => slot.to_string(),
        None => "None".to_string(),
      }
    )
  }
}