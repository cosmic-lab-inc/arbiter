use std::str::FromStr;
use anchor_lang::__private::bytemuck;
use base64::Engine;
use base64::engine::general_purpose;
use solana_account_decoder::UiAccount;
use solana_sdk::account::Account;
use solana_sdk::account_info::AccountInfo;
use solana_sdk::address_lookup_table::AddressLookupTableAccount;
use solana_sdk::pubkey::Pubkey;

const LOOKUP_TABLE_META_SIZE: usize = 56;

/// modified from sdk.1.17.x
/// https://docs.rs/solana-program/latest/src/solana_program/address_lookup_table/state.rs.html#192
pub fn deserialize_lookup_table(address: Pubkey, account: &Account) -> anyhow::Result<AddressLookupTableAccount> {
  let raw_addresses_data: &[u8] = account.data.get(LOOKUP_TABLE_META_SIZE..)
                                         .ok_or(anyhow::anyhow!("Invalid lookup table account"))?;
  let addresses = bytemuck::try_cast_slice(raw_addresses_data).map_err(|_| anyhow::anyhow!("Invalid lookup table account"))?;

  Ok(AddressLookupTableAccount {
    key: address,
    addresses: addresses.to_vec(),
  })
}

pub fn to_account(account: UiAccount, data: Vec<u8>) -> anyhow::Result<Account> {
  Ok(Account {
    lamports: account.lamports,
    data: general_purpose::STANDARD.decode(&data[..])?,
    owner: Pubkey::from_str(&account.owner)?,
    executable: account.executable,
    rent_epoch: account.rent_epoch
  })
}

pub fn to_account_info<'b>(key: Pubkey, signs: bool, writable: bool, exec: bool, acct: Account) -> AccountInfo<'b> {
  let key = Box::leak(Box::new(key));
  let data = Box::leak(Box::new(acct.data.clone()));
  AccountInfo::new(
    key,
    signs,
    writable,
    Box::leak(Box::new(acct.lamports)),
    data,
    Box::leak(Box::new(acct.owner)),
    exec,
    acct.rent_epoch,
  )
}