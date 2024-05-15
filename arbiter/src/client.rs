#![allow(unused_imports)]

use std::time::Duration;
use anchor_lang::prelude::AccountInfo;
use borsh::BorshDeserialize;
use reqwest::Client;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::account::Account;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use tokio::io::ReadBuf;

use common::*;
use nexus::{DriftClient, HistoricalPerformance, HistoricalSettlePnl, MarketInfo, Nexus};
use nexus::drift_cpi::{BASE_PRECISION, OrderParams, PositionDirection, PRICE_PRECISION};
use crate::Time;
use nexus::drift_cpi::drift;
use nexus::drift_cpi::ix_accounts::PlacePerpOrder;

pub struct Arbiter {
  signer: Keypair,
  pub nexus: Nexus,
}

impl Arbiter {
  pub async fn new(signer: Keypair, rpc: &str, wss: &str) -> anyhow::Result<Self> {
    Ok(Self {
      signer,
      nexus: Nexus::new(rpc, wss).await?
    })
  }

  /// Assumes .env contains key "WALLET" with keypair byte array. Example: `WALLET=[1,2,3,4,5]`
  ///
  /// Assumes .env contains key "RPC_URL" with HTTP endpoint.
  ///
  /// Assumes .env contains key "WS_URL" with WSS endpoint.
  pub async fn new_from_env() -> anyhow::Result<Self> {
    Ok(Self {
      signer: read_keypair_from_env("WALLET")?,
      nexus: Nexus::new_from_env().await?
    })
  }

  pub fn nexus(&self) -> &Nexus {
    &self.nexus
  }

  pub fn rpc(&self) -> &RpcClient {
    &self.nexus.rpc
  }

  pub fn client(&self) -> &Client {
    &self.nexus.client
  }

  pub fn log_order(&self, name: &str, params: &OrderParams, market_info: &MarketInfo) {
    let dir = match params.direction {
      PositionDirection::Long => "long",
      PositionDirection::Short => "short"
    };
    let oracle_price_offset = match params.oracle_price_offset {
      None => 0.0,
      Some(offset) => trunc!(offset as f64 / PRICE_PRECISION as f64, 2)
    };
    let base = trunc!(params.base_asset_amount as f64 / BASE_PRECISION as f64, 2);
    let limit_price = trunc!(market_info.price + oracle_price_offset, 2);
    println!(
      "{}, {} {} {} @ {} as {:?}",
      name,
      dir,
      base,
      market_info.name,
      limit_price,
      params.order_type
    );
  }

  fn to_account_info<'a>(key: &'a Pubkey, signs: bool, writable: bool, exec: bool, acct: &'a mut Account) -> AccountInfo<'a> {
    AccountInfo::new(
      key,
      signs,
      writable,
      &mut acct.lamports,
      &mut acct.data,
      &acct.owner,
      exec,
      acct.rent_epoch,
    )
  }

  // https://github.com/drift-labs/protocol-v2/blob/a3c2e276b3d6f9819eba7feaeb8737f0c61cb6ea/sdk/src/driftClient.ts#L2998

  // todo
  pub async fn copy_order(&self, params: OrderParams) -> anyhow::Result<()> {
    let signer_key = self.signer.pubkey();
    let state_key = DriftClient::state_pda();
    let user_key = DriftClient::user_pda(&self.signer.pubkey(), 0)?;
    let res = self.rpc().get_multiple_accounts_with_commitment(&[user_key, state_key, self.signer.pubkey()], CommitmentConfig::processed()).await?;
    let slot = res.context.slot;
    let mut user_acct = res.value[0].clone().ok_or(anyhow::anyhow!("User account not found"))?;
    let mut state_acct = res.value[1].clone().ok_or(anyhow::anyhow!("State account not found"))?;
    let mut signer_acct = res.value[2].clone().ok_or(anyhow::anyhow!("Signer account not found"))?;
    let user = Self::to_account_info(&user_key, false, true, false, &mut user_acct);
    let state = Self::to_account_info(&state_key, false, false, false, &mut state_acct);
    let signer = Self::to_account_info(&signer_key, true, true, false, &mut signer_acct);
    let authority = anchor_lang::prelude::Signer::try_from(&signer)?;

    let ix_accts = PlacePerpOrder {
      state,
      user,
      authority,
    };

    let base_asset_amount = 0;

    let mut params = params;
    params.base_asset_amount = base_asset_amount;
    let ctx = anchor_lang::context::Context::new();

    drift::place_perp_order(ix_accts, params);


    Ok(())
  }
}
