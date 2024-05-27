use std::cmp::Ordering;
use std::collections::HashMap;

use anchor_lang::prelude::{AccountInfo, AccountMeta};
use solana_sdk::pubkey::Pubkey;

use drift_cpi::{MarketType, OraclePriceData, OracleSource, User};

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
pub struct MarketInfo {
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
    let kind_eq = matches!((self.kind, other.kind), (MarketType::Spot, MarketType::Spot) | (MarketType::Perp, MarketType::Perp));
    index_eq && kind_eq
  }
}

impl MarketId {
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