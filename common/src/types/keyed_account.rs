use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;

#[derive(Clone)]
pub struct KeyedAccount<T: Clone> {
  pub key: Pubkey,
  pub account: T,
}

#[derive(Clone)]
pub struct AccountContext<T: Clone> {
  pub key: Pubkey,
  pub account: T,
  pub slot: u64
}

#[derive(Clone)]
pub struct DecodedAccountContext<T: Clone> {
  pub key: Pubkey,
  pub account: Account,
  pub slot: u64,
  pub decoded: T
}
