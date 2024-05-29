use solana_client::rpc_response::RpcConfirmedTransactionStatusWithSignature;
use solana_sdk::{
  clock::{Slot, UnixTimestamp},
  pubkey::Pubkey,
  signature::Signature,
  transaction::Transaction,
};
use solana_transaction_status::TransactionConfirmationStatus;
use std::time::Duration;

pub struct TrxData {
  pub tx: Transaction,
  pub signature: Signature,
  pub signer: Pubkey,
  pub slot: Slot,
  pub block_time: UnixTimestamp,
}

pub struct SignatureInfo {
  pub ctx: RpcConfirmedTransactionStatusWithSignature,
  pub unix_seconds_since: i64,
  pub formatted_time_since: String,
}

/// Config for [`RpcClientExt::confirm_transactions_with_config`]
#[derive(Debug)]
pub struct ConfirmTransactionConfig {
  /// How often to check for confirmations
  pub loop_rate: Duration,
  /// The minimum confirmation status to wait for
  pub min_confirmation: TransactionConfirmationStatus,
  /// This number of iterations * loop_rate = total duration to wait for confirmation
  pub max_confirmation_checks: u16,
}

impl Default for ConfirmTransactionConfig {
  fn default() -> Self {
    Self {
      loop_rate: Duration::from_millis(200),
      min_confirmation: TransactionConfirmationStatus::Confirmed,
      max_confirmation_checks: 100,
    }
  }
}
