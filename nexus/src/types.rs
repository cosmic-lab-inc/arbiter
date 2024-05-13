use futures::future::BoxFuture;
use futures::stream::BoxStream;
use solana_sdk::pubkey::Pubkey;
use serde::{Deserialize, Serialize};
use solana_sdk::transaction::{TransactionError, TransactionVersion};
use solana_transaction_status::{EncodedTransaction, EncodedTransactionWithStatusMeta, Rewards, UiInnerInstructions, UiLoadedAddresses, UiTransactionTokenBalance};

pub type StreamUnsub = Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send>;
pub type EventStream<'a, T> = BoxStream<'a, T>;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum NexusWebsocketEvent {
  Msg(serde_json::Value)
}

pub type Callback = Box<dyn Fn(NexusWebsocketEvent) -> anyhow::Result<()> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Default, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionSubscribeFilter {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub vote: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub failed: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub signature: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub account_include: Option<Vec<String>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub account_exclude: Option<Vec<String>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub account_required: Option<Vec<String>>,
}

impl TransactionSubscribeFilter {
  pub fn standard(key: &Pubkey) -> Self {
    Self {
      account_include: Some(vec![key.to_string()]),
      ..Default::default()
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransactionCommitment {
  Processed,
  Confirmed,
  Finalized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UiTransactionEncoding {
  Binary, // Legacy. Retained for RPC backwards compatibility
  Base58,
  Base64,
  #[serde(rename = "base64+zstd")]
  Base64Zstd,
  JsonParsed
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransactionDetails {
  Full,
  Signatures,
  Accounts,
  None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionSubscribeOptions {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub commitment: Option<TransactionCommitment>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub encoding: Option<UiTransactionEncoding>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub transaction_details: Option<TransactionDetails>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub show_rewards: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub max_supported_transaction_version: Option<u8>
}

impl Default for TransactionSubscribeOptions {
  fn default() -> Self {
    Self {
      commitment: Some(TransactionCommitment::Confirmed),
      encoding: Some(UiTransactionEncoding::JsonParsed),
      transaction_details: Some(TransactionDetails::Full),
      show_rewards: Some(true),
      max_supported_transaction_version: Some(0)
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcTransactionsConfig {
  pub filter: TransactionSubscribeFilter,
  pub options: TransactionSubscribeOptions
}

// Websocket transaction response

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionNotification {
  pub transaction: EncodedTransactionWithStatusMeta,
  pub signature: String
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionNotificationWithMeta {
  pub transaction: EncodedTransaction,
  pub meta: TransactionNotificationMeta,
  pub version: TransactionVersion
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionNotificationMeta {
  #[serde(default)]
  pub err: Option<TransactionError>,
  pub status: solana_sdk::transaction::Result<()>, // This field is deprecated.  See https://github.com/solana-labs/solana/issues/9302
  pub fee: u64,
  pub pre_balances: Vec<u64>,
  pub post_balances: Vec<u64>,
  pub inner_instructions: Option<Vec<UiInnerInstructions>>,
  pub log_messages: Option<Vec<String>>,
  pub pre_token_balances: Option<Vec<UiTransactionTokenBalance>>,
  pub post_token_balances: Option<Vec<UiTransactionTokenBalance>>,
  pub rewards: Rewards,
  pub loaded_addresses: UiLoadedAddresses,
  // #[serde(default)]
  // pub return_data: Option<UiTransactionReturnData>,
  pub compute_units_consumed: u64,
}