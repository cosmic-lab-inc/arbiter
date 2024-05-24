use anchor_lang::__private::bytemuck;
use anchor_lang::AccountDeserialize;
use base64::Engine;
use base64::engine::general_purpose;
use solana_account_decoder::UiAccountData;
use solana_sdk::account::Account;
use solana_sdk::address_lookup_table::AddressLookupTableAccount;
use solana_sdk::pubkey::Pubkey;

const LOOKUP_TABLE_META_SIZE: usize = 56;

/// modified from sdk.1.17.x
/// https://docs.rs/solana-program/latest/src/solana_program/address_lookup_table/state.rs.html#192
pub fn deserialize_lookup_table(address: Pubkey, account: &Account) -> anyhow::Result<AddressLookupTableAccount> {
  let raw_addresses_data: &[u8] = account.data.get(LOOKUP_TABLE_META_SIZE..).ok_or(anyhow::anyhow!("Invalid lookup table account"))?;
  let addresses = bytemuck::try_cast_slice(raw_addresses_data).map_err(|_| anyhow::anyhow!("Invalid lookup table account"))?;

  Ok(AddressLookupTableAccount {
    key: address,
    addresses: addresses.to_vec(),
  })
}

pub fn decode_ui_account<T: AccountDeserialize>(data: UiAccountData) -> anyhow::Result<T> {
  let data_str = match data {
    UiAccountData::Binary(encoded, _) => encoded,
    _ => return Err(anyhow::anyhow!("Unsupported UiAccountData encoding")),
  };
  let decoded_data = general_purpose::STANDARD.decode(data_str.as_bytes())?;
  let mut decoded_data_slice = decoded_data.as_slice();
  T::try_deserialize(&mut decoded_data_slice).map_err(|e| anyhow::anyhow!("{:?}", e))
}