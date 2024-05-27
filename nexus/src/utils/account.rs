use std::str::FromStr;

use anchor_lang::AccountDeserialize;
use base64::Engine;
use base64::engine::general_purpose;
use solana_account_decoder::{UiAccount, UiAccountData};
use solana_sdk::account::Account;
use solana_sdk::account_info::AccountInfo;
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::SubscribeUpdateAccountInfo;

use crate::AcctCtx;

pub trait ToAccount {
  fn to_account(&self) -> anyhow::Result<Account>;
}

pub trait ToAccountInfo {
  fn to_account_info<'b>(&self, key: Pubkey, signs: bool, writable: bool, exec: bool) -> AccountInfo<'b>;
}

pub trait DecodeAccount {
  fn decode_account<T: AccountDeserialize>(&self) -> anyhow::Result<T>;
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

impl ToAccountInfo for AcctCtx {
  fn to_account_info<'b>(&self, _: Pubkey, signs: bool, writable: bool, exec: bool) -> AccountInfo<'b> {
    self.account.to_account_info(self.key, signs, writable, exec)
  }
}

impl ToAccount for SubscribeUpdateAccountInfo {
  fn to_account(&self) -> anyhow::Result<Account> {
    Ok(Account {
      lamports: self.lamports,
      data: self.data.clone(),
      owner: Pubkey::try_from(self.owner.clone()).map_err(|_| anyhow::anyhow!("Failed to convert owner to pubkey"))?,
      executable: self.executable,
      rent_epoch: self.rent_epoch,
    })
  }
}

impl DecodeAccount for Account {
  fn decode_account<T: AccountDeserialize>(&self) -> anyhow::Result<T> {
    let mut decoded_data_slice = self.data.as_slice();
    T::try_deserialize(&mut decoded_data_slice).map_err(|e| anyhow::anyhow!("{:?}", e))
  }
}

impl DecodeAccount for UiAccount {
  fn decode_account<T: AccountDeserialize>(&self) -> anyhow::Result<T> {
    let data_str = match &self.data {
      UiAccountData::Binary(encoded, _) => encoded,
      _ => return Err(anyhow::anyhow!("Unsupported UiAccountData encoding")),
    };
    let decoded_data = general_purpose::STANDARD.decode(data_str.as_bytes())?;
    let mut decoded_data_slice = decoded_data.as_slice();
    T::try_deserialize(&mut decoded_data_slice).map_err(|e| anyhow::anyhow!("{:?}", e))
  }
}

