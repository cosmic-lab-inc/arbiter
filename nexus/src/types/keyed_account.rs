use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;

#[derive(Clone)]
pub struct AcctCtx {
  pub key: Pubkey,
  pub account: Account,
  pub slot: u64
}

#[derive(Clone)]
pub struct DecodedAcctCtx<T: Clone> {
  pub key: Pubkey,
  pub account: Account,
  pub slot: u64,
  pub decoded: T
}
