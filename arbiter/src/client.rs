#![allow(unused_imports)]

use std::time::Duration;
use borsh::BorshDeserialize;
use reqwest::Client;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use tokio::io::ReadBuf;

use common::*;
use nexus::{HistoricalPerformance, HistoricalSettlePnl, Nexus};
use crate::Time;

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
}
