use anchor_lang::solana_program::address_lookup_table::AddressLookupTableAccount;
use solana_account_decoder::UiAccountEncoding;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::{
  RpcSimulateTransactionAccountsConfig, RpcSimulateTransactionConfig,
};
use solana_rpc_client_api::config::{RpcSendTransactionConfig, RpcTransactionConfig};
use solana_rpc_client_api::response::{Response, RpcSimulateTransactionResult};
use solana_sdk::clock::Slot;
use solana_sdk::commitment_config::{CommitmentConfig, CommitmentLevel};
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::instruction::Instruction;
use solana_sdk::message::{v0, Message, VersionedMessage};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::signer::Signer;
use solana_sdk::signers::Signers;
use solana_sdk::transaction::VersionedTransaction;
use solana_transaction_status::{
  TransactionConfirmationStatus, TransactionStatus, UiTransactionEncoding,
};
use spl_token::solana_program::native_token::LAMPORTS_PER_SOL;
use std::sync::Arc;
use tokio::time::MissedTickBehavior;

use crate::{trunc, ConfirmTransactionConfig, MICRO_LAMPORTS_PER_LAMPORT};

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

pub struct TrxBuilder<'a, S: Signer + Sized, T: Signers> {
  rpc: Arc<RpcClient>,
  /// ordered list of instructions
  ixs: Vec<Instruction>,
  /// use legacy transaction mode
  legacy: bool,
  /// add additional lookup tables (v0 only)
  lookup_tables: Vec<AddressLookupTableAccount>,
  prior_fee_added: bool,
  payer: &'a S,
  signers: T,
}

impl<'a, S: Signer + Sized, T: Signers> TrxBuilder<'a, S, T> {
  pub fn new(
    rpc: Arc<RpcClient>,
    legacy: bool,
    lookup_tables: Vec<AddressLookupTableAccount>,
    payer: &'a S,
    signers: T,
  ) -> Self {
    Self {
      rpc,
      ixs: vec![],
      legacy,
      lookup_tables,
      prior_fee_added: false,
      payer,
      signers,
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

  async fn recent_priority_fee(&self, key: Pubkey, window: Option<usize>) -> anyhow::Result<u64> {
    let response = self.rpc.get_recent_prioritization_fees(&[key]).await?;
    let window = window.unwrap_or(100);
    let fees: Vec<u64> = response
      .iter()
      .take(window)
      .map(|x| x.prioritization_fee)
      .collect();
    Ok(fees.iter().sum::<u64>() / fees.len() as u64)
  }

  /// Set the priority fee of the tx
  ///
  /// `microlamports_per_cu` the price per unit of compute in µ-lamports
  pub async fn with_priority_fee(
    &mut self,
    key: Pubkey,
    window: Option<usize>,
    cu_limit: Option<u32>,
  ) -> anyhow::Result<()> {
    let ul_per_cu = self
      .recent_priority_fee(key, window)
      .await?
      .max(MICRO_LAMPORTS_PER_LAMPORT);
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
  pub async fn build(&self) -> anyhow::Result<VersionedTransaction> {
    let bh = self.rpc.get_latest_blockhash().await?;
    let msg = if self.legacy {
      VersionedMessage::Legacy(Message::new_with_blockhash(
        self.ixs.as_ref(),
        Some(&self.payer.pubkey()),
        &bh,
      ))
    } else {
      VersionedMessage::V0(v0::Message::try_compile(
        &self.payer.pubkey(),
        self.ixs.as_slice(),
        self.lookup_tables.as_slice(),
        bh,
      )?)
    };
    let tx = VersionedTransaction::try_new(msg, &self.signers)?;
    Ok(tx)
  }

  pub async fn compute_units(&mut self) -> anyhow::Result<u32> {
    let tx = self.build().await?;
    let sim = self.rpc.simulate_transaction(&tx).await?;
    Ok(
      sim
        .value
        .units_consumed
        .ok_or(anyhow::anyhow!("No compute units found"))? as u32,
    )
  }

  fn log_priority_fee(ul_per_cu: u64, cu_limit: Option<u32>) {
    match cu_limit {
      Some(cu_limit) => {
        let ul_cost = ul_per_cu * cu_limit as u64;
        let sol_cost = ul_cost as f64 / MICRO_LAMPORTS_PER_LAMPORT as f64 / LAMPORTS_PER_SOL as f64;
        log::debug!("Priority fee: {} SOL", trunc!(sol_cost, 2));
      }
      None => {
        log::debug!("Priority fee: {} µ-lamports per compute unit", ul_per_cu);
      }
    }
  }

  pub async fn simulate(
    &mut self,
    prior_fee_key: Pubkey,
  ) -> anyhow::Result<Response<RpcSimulateTransactionResult>> {
    if !self.prior_fee_added {
      self.with_priority_fee(prior_fee_key, None, None).await?;
    }

    let tx = self.build().await?;
    let config = RpcSimulateTransactionConfig {
      commitment: Some(CommitmentConfig::processed()),
      encoding: Some(UiTransactionEncoding::Base64),
      accounts: Some(RpcSimulateTransactionAccountsConfig {
        encoding: Some(UiAccountEncoding::Base64),
        addresses: vec![],
      }),
      ..Default::default()
    };
    let sim = self
      .rpc
      .simulate_transaction_with_config(&tx, config)
      .await?;
    log::info!("Simulation: {:#?}", sim.value);
    Ok(sim)
  }

  pub async fn send(
    &mut self,
    prior_fee_key: Pubkey,
    cu_limit: Option<u32>,
  ) -> anyhow::Result<(Signature, TransactionResult<Slot>)> {
    if !self.prior_fee_added {
      self
        .with_priority_fee(prior_fee_key, None, cu_limit)
        .await?;
    }

    let config = RpcSendTransactionConfig {
      skip_preflight: true,
      max_retries: Some(0),
      preflight_commitment: Some(CommitmentLevel::Confirmed),
      ..Default::default()
    };

    let tx = self.build().await?;

    let now = std::time::Instant::now();
    // todo: monitor our transactions with grpc, and break loop if cache has confirmed our transaction
    let sig = match self.rpc.send_transaction_with_config(&tx, config).await {
      Ok(sig) => Ok(sig),
      Err(e) => {
        log::error!("Failed to send transaction: {:#?}", e);
        Err(anyhow::anyhow!(e))
      }
    }?;
    for _ in 0..10 {
      let rpc = self.rpc.clone();
      let tx = tx.clone();
      tokio::task::spawn(async move {
        match rpc.send_transaction_with_config(&tx, config).await {
          Ok(sig) => Ok(sig),
          Err(e) => {
            log::error!("Failed to send transaction: {:#?}", e);
            Err(anyhow::anyhow!(e))
          }
        }?;
        Result::<_, anyhow::Error>::Ok(())
      });
    }
    Self::log_tx(&sig);

    let res = self
      .confirm(sig, ConfirmTransactionConfig::default())
      .await?;
    if res.is_ok() {
      log::warn!("Transaction confirmed in {:?}", now.elapsed());
    }
    Ok((sig, res))
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

  fn tc_into_commitment(confirmation: &TransactionConfirmationStatus) -> CommitmentConfig {
    match confirmation {
      TransactionConfirmationStatus::Processed => CommitmentConfig::processed(),
      TransactionConfirmationStatus::Confirmed => CommitmentConfig::confirmed(),
      TransactionConfirmationStatus::Finalized => CommitmentConfig::finalized(),
    }
  }

  async fn confirm(
    &self,
    sig: Signature,
    config: ConfirmTransactionConfig,
  ) -> anyhow::Result<TransactionResult<Slot>> {
    let mut result: Option<TransactionResult<Slot>> = None;
    let mut interval = tokio::time::interval(config.loop_rate);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut not_resolved = true;
    let mut iterations = 0;
    while not_resolved && iterations < config.max_confirmation_checks {
      interval.tick().await;
      let res = self
        .rpc
        .get_signature_statuses(vec![sig].as_slice())
        .await?;
      let status = res
        .value
        .first()
        .ok_or(anyhow::anyhow!("Failed to get signature from response"))?
        .clone();
      match status {
        Some(TransactionStatus {
          slot,
          err,
          confirmation_status: Some(confirmation_status),
          ..
        }) if Self::confirmation_at_least(&config.min_confirmation, &confirmation_status) => {
          result = Some(match err {
            None => Ok(slot),
            Some(error) => {
              let tx = match self
                .rpc
                .get_transaction_with_config(
                  &sig,
                  RpcTransactionConfig {
                    encoding: None,
                    commitment: Some(Self::tc_into_commitment(&config.min_confirmation)),
                    max_supported_transaction_version: Some(0),
                  },
                )
                .await
              {
                Ok(res) => res,
                Err(_) => {
                  self
                    .rpc
                    .get_transaction_with_config(
                      &sig,
                      RpcTransactionConfig {
                        encoding: None,
                        commitment: Some(Self::tc_into_commitment(&config.min_confirmation)),
                        max_supported_transaction_version: Some(0),
                      },
                    )
                    .await?
                }
              };
              Err(TxError::TxError {
                slot,
                error,
                logs: tx
                  .transaction
                  .meta
                  .and_then(|meta| meta.log_messages.into()),
              })
            }
          });
          not_resolved = false;
        }
        _ => {}
      }
      iterations += 1;
    }
    Ok(if result.is_none() {
      Err(TxError::Dropped)
    } else {
      result.ok_or(anyhow::anyhow!("Failed to define transaction state"))?
    })
  }
}
