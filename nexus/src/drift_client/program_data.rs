use solana_sdk::address_lookup_table::AddressLookupTableAccount;
use solana_sdk::pubkey::Pubkey;
use drift_cpi::{PerpMarket, SpotMarket};

/// Static-ish metadata from onchain drift program
#[derive(Clone)]
pub struct ProgramData {
  spot_markets: &'static [SpotMarket],
  perp_markets: &'static [PerpMarket],
  pub lookup_table: AddressLookupTableAccount,
}

impl ProgramData {
  /// Return an uninitialized instance of `ProgramData` (useful for bootstrapping)
  pub const fn uninitialized() -> Self {
    Self {
      spot_markets: &[],
      perp_markets: &[],
      lookup_table: AddressLookupTableAccount {
        key: Pubkey::new_from_array([0; 32]),
        addresses: vec![],
      },
    }
  }
  /// Initialize `ProgramData`
  pub fn new(
    mut spot: Vec<SpotMarket>,
    mut perp: Vec<PerpMarket>,
    lookup_table: AddressLookupTableAccount,
  ) -> Self {
    spot.sort_by(|a, b| a.market_index.cmp(&b.market_index));
    perp.sort_by(|a, b| a.market_index.cmp(&b.market_index));
    // other code relies on aligned indexes for fast lookups
    assert!(
      spot.iter()
          .enumerate()
          .all(|(idx, x)| idx == x.market_index as usize),
      "spot indexes unaligned"
    );
    assert!(
      perp.iter()
          .enumerate()
          .all(|(idx, x)| idx == x.market_index as usize),
      "perp indexes unaligned"
    );

    Self {
      spot_markets: Box::leak(spot.into_boxed_slice()),
      perp_markets: Box::leak(perp.into_boxed_slice()),
      lookup_table,
    }
  }

  /// Return known spot markets
  pub fn spot_market_configs(&self) -> &'static [SpotMarket] {
    self.spot_markets
  }

  /// Return known perp markets
  pub fn perp_market_configs(&self) -> &'static [PerpMarket] {
    self.perp_markets
  }

  /// Return the spot market config given a market index
  pub fn spot_market_config_by_index(&self, market_index: u16) -> Option<&'static SpotMarket> {
    self.spot_markets.get(market_index as usize)
  }

  /// Return the perp market config given a market index
  pub fn perp_market_config_by_index(&self, market_index: u16) -> Option<&'static PerpMarket> {
    self.perp_markets.get(market_index as usize)
  }
}