use std::collections::HashMap;

use solana_sdk::clock::Slot;
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::{CommitmentLevel, SubscribeRequest, SubscribeRequestFilterAccounts, SubscribeRequestFilterBlocksMeta, SubscribeRequestFilterSlots, SubscribeRequestFilterTransactions};

use crate::Time;

#[derive(Clone)]
pub struct GeyserConfig {
  pub grpc: String,
  pub x_token: String,
  pub slots: Option<SubscribeRequestFilterSlots>,
  pub accounts: Option<SubscribeRequestFilterAccounts>,
  pub transactions: Option<SubscribeRequestFilterTransactions>,
  pub blocks_meta: Option<SubscribeRequestFilterBlocksMeta>,
  pub commitment: CommitmentLevel,
}

impl Default for GeyserConfig {
  fn default() -> Self {
    Self {
      grpc: "".to_string(),
      x_token: "".to_string(),
      slots: Some(SubscribeRequestFilterSlots { filter_by_commitment: Some(false) }),
      accounts: None,
      transactions: None,
      blocks_meta: None,
      commitment: CommitmentLevel::Processed,
    }
  }
}

impl From<GeyserConfig> for SubscribeRequest {
  fn from(value: GeyserConfig) -> SubscribeRequest {
    SubscribeRequest {
      slots: match value.slots {
        Some(filter) => maplit::hashmap! { "".to_owned() => filter },
        None => HashMap::new()
      },
      accounts: match value.accounts {
        Some(filter) => maplit::hashmap! { "".to_owned() => filter },
        None => HashMap::new(),
      },
      transactions: match value.transactions {
        Some(filter) => maplit::hashmap! { "".to_owned() => filter },
        None => HashMap::new()
      },
      transactions_status: HashMap::new(),
      entry: HashMap::new(),
      blocks: HashMap::new(),
      blocks_meta: match value.blocks_meta {
        Some(filter) => maplit::hashmap! { "".to_owned() => filter },
        None => HashMap::new(),
      },
      commitment: Some(value.commitment as i32),
      accounts_data_slice: vec![],
      ping: None,
    }
  }
}

#[derive(Debug)]
pub struct Ix {
  pub program: Pubkey,
  pub accounts: Vec<Pubkey>,
  pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct TxStub {
  pub ixs: Vec<Ix>,
  pub slot: Slot,
  pub blockhash: String,
}

#[derive(Debug, Clone)]
pub struct BlockInfo {
  pub slot: Slot,
  pub blockhash: String,
  pub time: Time,
}