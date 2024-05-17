use nexus::drift_cpi::{PerpMarket, SpotMarket, User};
use solana_account_decoder::UiAccount;
use common::AccountContext;

#[allow(clippy::large_enum_variant)]
pub enum ChannelEvent {
  PerpMarket(AccountContext<PerpMarket>),
  SpotMarket(AccountContext<SpotMarket>),
  User(AccountContext<User>),
  Oracle(AccountContext<UiAccount>)
}