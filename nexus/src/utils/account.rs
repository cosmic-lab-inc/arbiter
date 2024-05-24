use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose;
use solana_account_decoder::{UiAccount, UiAccountData};
use solana_sdk::account::Account;
use solana_sdk::account_info::AccountInfo;
use solana_sdk::pubkey::Pubkey;

pub trait ToAccount {
  fn to_account(&self) -> anyhow::Result<Account>;
}

pub trait ToAccountInfo {
  fn to_account_info<'b>(&self, key: Pubkey, signs: bool, writable: bool, exec: bool) -> AccountInfo<'b>;
}

impl ToAccount for UiAccount {
  fn to_account(&self) -> anyhow::Result<Account> {
    let data_str = match &self.data {
      UiAccountData::Binary(encoded, _) => encoded,
      _ => return Err(anyhow::anyhow!("Unsupported UiAccountData encoding")),
    };
    Ok(Account {
      lamports: self.lamports,
      data: general_purpose::STANDARD.decode(data_str.as_bytes())?,
      owner: Pubkey::from_str(&self.owner)?,
      executable: self.executable,
      rent_epoch: self.rent_epoch,
    })
  }
}

impl ToAccountInfo for Account {
  fn to_account_info<'b>(&self, key: Pubkey, signs: bool, writable: bool, exec: bool) -> AccountInfo<'b> {
    let key = Box::leak(Box::new(key));
    let data = Box::leak(Box::new(self.data.clone()));
    AccountInfo::new(
      key,
      signs,
      writable,
      Box::leak(Box::new(self.lamports)),
      data,
      Box::leak(Box::new(self.owner)),
      exec,
      self.rent_epoch,
    )
  }
}

