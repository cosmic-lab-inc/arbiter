#![allow(unused_imports)]

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signature::Keypair;

use common::*;

pub struct ArbiterClient {
  pub signer: Keypair,
  pub rpc: RpcClient,
}

impl ArbiterClient {
  pub async fn new(signer: Keypair, rpc_url: String) -> anyhow::Result<Self> {
    Ok(Self {
      signer,
      rpc: RpcClient::new(rpc_url),
    })
  }

  pub fn rpc(&self) -> &RpcClient {
    &self.rpc
  }

  pub fn read_keypair_from_env(env_key: &str) -> anyhow::Result<Keypair> {
    read_keypair_from_env(env_key)
  }
}