#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use anchor_lang::solana_program::address_lookup_table::AddressLookupTableAccount;
use solana_account_decoder::UiAccountEncoding;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_client::SerializableTransaction;
use solana_client::rpc_config::{RpcSimulateTransactionAccountsConfig, RpcSimulateTransactionConfig};
use solana_rpc_client_api::config::{RpcSendTransactionConfig, RpcTransactionConfig};
use solana_rpc_client_api::response::{Response, RpcSimulateTransactionResult};
use solana_sdk::clock::Slot;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::instruction::Instruction;
use solana_sdk::message::{Message, v0, VersionedMessage};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::signer::Signer;
use solana_sdk::signers::Signers;
use solana_sdk::transaction::VersionedTransaction;
use solana_transaction_status::{TransactionConfirmationStatus, TransactionStatus, UiTransactionEncoding};
use spl_token::solana_program::native_token::LAMPORTS_PER_SOL;
use tokio::time::MissedTickBehavior;

use crate::ConfirmTransactionConfig;

/// Error received when confirming a transaction
#[derive(Debug, thiserror::Error)]
pub enum TxError {
  /// The transaction was confirmed with an error
  #[error("Transaction confirmed in slot `{slot}` with error: {error}.\nLogs: {logs:#?}")]
  TxError {
    /// The slot we confirmed the transaction in
    slot: Slot,
    /// The error that occurred
    error: solana_sdk::transaction::TransactionError,
    /// The transaction logs.
    logs: Option<Vec<String>>,
  },
  /// The transaction was dropped
  #[error("Transaction was dropped")]
  Dropped,
}

/// The result of a transaction
pub type TransactionResult<T> = Result<T, TxError>;

pub struct TrxBuilder {
  rpc: Arc<RpcClient>,
  /// ordered list of instructions
  ixs: Vec<Instruction>,
  /// use legacy transaction mode
  legacy: bool,
  /// add additional lookup tables (v0 only)
  lookup_tables: Vec<AddressLookupTableAccount>,
  pub prior_fee_added: bool,
}

impl TrxBuilder {
  pub fn new(rpc: Arc<RpcClient>, legacy: bool, lookup_tables: Vec<AddressLookupTableAccount>) -> Self {
    Self {
      rpc,
      ixs: vec![],
      legacy,
      lookup_tables,
      prior_fee_added: false,
    }
  }

  pub fn ixs(&self) -> &[Instruction] {
    &self.ixs
  }

  pub fn with_ixs(mut self, ixs: Vec<Instruction>) -> Self {
    self.ixs = ixs;
    self
  }

  pub fn is_empty(&self) -> bool {
    if self.ixs().is_empty() {
      true
    } else {
      // only empty if there are instructions besides compute budget program
      let mut is_empty = true;
      for ix in self.ixs().iter() {
        if ix.program_id != solana_sdk::compute_budget::id() {
          is_empty = false;
        }
      }
      is_empty
    }
  }

  async fn recent_priority_fee(
    &self,
    key: Pubkey,
    window: Option<usize>,
  ) -> anyhow::Result<u64> {
    let response = self.rpc.get_recent_prioritization_fees(&[key]).await?;
    let window = window.unwrap_or(100);
    let fees: Vec<u64> = response.iter().take(window).map(|x| x.prioritization_fee).collect();
    Ok(fees.iter().sum::<u64>() / fees.len() as u64)
  }

  /// Set the priority fee of the tx
  ///
  /// `microlamports_per_cu` the price per unit of compute in µ-lamports
  pub async fn with_priority_fee(&mut self, key: Pubkey, window: Option<usize>, cu_limit: Option<u32>) -> anyhow::Result<()> {
    let ul_per_cu = self.recent_priority_fee(key, window).await?.max(10_000);
    let cu_limit_ix = ComputeBudgetInstruction::set_compute_unit_price(ul_per_cu);
    self.ixs.insert(0, cu_limit_ix);
    if let Some(cu_limit) = cu_limit {
      let cu_price_ix = ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);
      self.ixs.insert(1, cu_price_ix);
    }
    Self::log_priority_fee(ul_per_cu, cu_limit);
    self.prior_fee_added = true;
    Ok(())
  }

  pub fn reset_ixs(&mut self) {
    self.ixs.clear();
  }

  /// Use legacy tx mode
  pub fn legacy(mut self) -> Self {
    self.legacy = true;
    self
  }

  pub fn add_ixs(&mut self, ixs: Vec<Instruction>) {
    self.ixs.extend(ixs);
  }

  pub fn log_tx(sig: &Signature) {
    let url = "https://solana.fm/tx/";
    log::info!("Signature: {}{}", url, sig)
  }

  /// Build the transaction message ready for signing and sending
  pub async fn build<S: Signer + Sized, T: Signers>(&self, payer: &S, signers: &T) -> anyhow::Result<VersionedTransaction> {
    let bh = self.rpc.get_latest_blockhash().await?;
    let msg = if self.legacy {
      VersionedMessage::Legacy(Message::new_with_blockhash(
        self.ixs.as_ref(),
        Some(&payer.pubkey()),
        &bh,
      ))
    } else {
      VersionedMessage::V0(v0::Message::try_compile(
        &payer.pubkey(),
        self.ixs.as_slice(),
        self.lookup_tables.as_slice(),
        bh,
      )?)
    };
    let tx = VersionedTransaction::try_new(
      msg,
      signers,
    )?;
    Ok(tx)
  }

  pub async fn compute_units<S: Signer + Sized, T: Signers>(&mut self, payer: &S, signers: &T) -> anyhow::Result<u32> {
    let tx = self.build(payer, signers).await?;
    let sim = self.rpc.simulate_transaction(&tx).await?;
    Ok(sim.value.units_consumed.ok_or(anyhow::anyhow!("No compute units found"))? as u32)
  }

  fn log_priority_fee(ul_per_cu: u64, cu_limit: Option<u32>) {
    match cu_limit {
      Some(cu_limit) => {
        let ul_cost = ul_per_cu * cu_limit as u64;
        let lamport_cost = ul_cost / 1_000_000;
        let sol_cost = lamport_cost as f64 / LAMPORTS_PER_SOL as f64;
        log::debug!("Priority fee: {} SOL", sol_cost);
      }
      None => {
        log::debug!("Priority fee: {} u-lamports per compute unit", ul_per_cu);
      }
    }
  }

  pub async fn simulate<S: Signer + Sized, T: Signers>(
    &mut self,
    payer: &S,
    signers: &T,
    prior_fee_key: Pubkey,
  ) -> anyhow::Result<Response<RpcSimulateTransactionResult>> {
    if !self.prior_fee_added {
      self.with_priority_fee(prior_fee_key, None, None).await?;
    }

    let tx = self.build(payer, signers).await?;
    let config = RpcSimulateTransactionConfig {
      commitment: Some(CommitmentConfig::processed()),
      encoding: Some(UiTransactionEncoding::Base64),
      accounts: Some(RpcSimulateTransactionAccountsConfig {
        encoding: Some(UiAccountEncoding::Base64),
        addresses: vec![],
      }),
      ..Default::default()
    };
    let sim = self.rpc.simulate_transaction_with_config(&tx, config).await?;
    log::debug!("Simulated transaction: {:#?}", sim.value);
    Ok(sim)
  }

  pub async fn send<S: Signer + Sized, T: Signers>(&mut self, payer: &S, signers: &T, prior_fee_key: Pubkey) -> anyhow::Result<Signature> {
    if !self.prior_fee_added {
      self.with_priority_fee(prior_fee_key, None, None).await?;
    }

    let config = RpcSendTransactionConfig {
      skip_preflight: false,
      ..Default::default()
    };

    const SEND_RETRIES: usize = 1;
    const GET_STATUS_RETRIES: usize = 10; // 10 * 500 millis = 5 seconds
    let tx = self.build(payer, signers).await?;
    'sending: for _ in 0..SEND_RETRIES {
      let sig = match self.rpc.send_transaction_with_config(&tx, config).await {
        Ok(sig) => Ok(sig),
        Err(e) => {
          log::error!("Failed to send transaction: {:#?}", e);
          Err(anyhow::anyhow!(e))
        }
      }?;
      Self::log_tx(&sig);
      let rbh = *tx.get_recent_blockhash();
      for _status_retry in 0..GET_STATUS_RETRIES {
        match self.rpc.get_signature_status(&sig).await? {
          Some(Ok(_)) => return Ok(sig),
          Some(Err(e)) => {
            log::error!("Failed transaction: {:#?}", e);
            return Err(e.into());
          }
          None => {
            if !self.rpc.is_blockhash_valid(&rbh, CommitmentConfig::processed()).await?
            {
              // Block hash is not found by some reason
              break 'sending;
            } else if cfg!(not(test))
            // Ignore sleep at last step. && status_retry < GET_STATUS_RETRIES
            {
              // Retry twice a second
              tokio::time::sleep(std::time::Duration::from_millis(500)).await;
              continue;
            }
          }
        }
      }
    }
    Err(anyhow::anyhow!("Transaction dropped"))
  }

  fn confirmation_at_least(
    control: &TransactionConfirmationStatus,
    test: &TransactionConfirmationStatus,
  ) -> bool {
    matches!(
        (control, test),
        (TransactionConfirmationStatus::Processed, _)
            | (
                TransactionConfirmationStatus::Confirmed,
                TransactionConfirmationStatus::Confirmed | TransactionConfirmationStatus::Finalized,
            )
            | (
                TransactionConfirmationStatus::Finalized,
                TransactionConfirmationStatus::Finalized
            )
    )
  }

  /// Convert a [`TransactionConfirmationStatus`] into a [`CommitmentConfig`]
  #[must_use]
  pub fn tc_into_commitment(confirmation: &TransactionConfirmationStatus) -> CommitmentConfig {
    match confirmation {
      TransactionConfirmationStatus::Processed => CommitmentConfig::processed(),
      TransactionConfirmationStatus::Confirmed => CommitmentConfig::confirmed(),
      TransactionConfirmationStatus::Finalized => CommitmentConfig::finalized(),
    }
  }

  async fn confirm(
    &self,
    sig_and_block_height: impl IntoIterator<Item=(Signature, u64)> + Send,
    config: ConfirmTransactionConfig,
  ) -> anyhow::Result<HashMap<Signature, TransactionResult<Slot>>> {
    let mut sigs = Vec::new();
    let mut block_heights = Vec::new();
    for (sig, block_height) in sig_and_block_height {
      sigs.push(sig);
      block_heights.push(block_height);
    }
    let mut out = HashMap::new();
    let mut interval = tokio::time::interval(config.loop_rate);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    while !sigs.is_empty() {
      interval.tick().await;
      let block_height = self.rpc.get_block_height_with_commitment(Self::tc_into_commitment(&config.min_confirmation)).await?;
      let statues = self.rpc.get_signature_statuses(&sigs).await?;
      for (index, status) in statues.value.into_iter().enumerate().rev() {
        match status {
          Some(TransactionStatus {
                 slot,
                 err,
                 confirmation_status: Some(confirmation_status),
                 ..
               }) if Self::confirmation_at_least(&config.min_confirmation, &confirmation_status) => {
            let sig = sigs[index];
            out.insert(
              sig,
              match err {
                None => Ok(slot),
                Some(error) => {
                  let tx = self.rpc.get_transaction_with_config(
                    &sig,
                    RpcTransactionConfig {
                      encoding: None,
                      commitment: Some(Self::tc_into_commitment(
                        &config.min_confirmation,
                      )),
                      max_supported_transaction_version: None,
                    },
                  ).await?;
                  Err(TxError::TxError {
                    slot,
                    error,
                    logs: tx.transaction.meta.and_then(|meta| meta.log_messages.into()),
                  })
                }
              },
            );
            sigs.swap_remove(index);
            block_heights.swap_remove(index);
          }
          _ => {}
        }
      }

      for (index, last_block_height) in block_heights.clone().into_iter().enumerate().rev() {
        if last_block_height < block_height {
          out.insert(sigs[index], Err(TxError::Dropped));
          sigs.swap_remove(index);
          block_heights.swap_remove(index);
        }
      }
    }
    Ok(out)
  }
}