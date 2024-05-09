#![allow(unused_imports)]

use std::sync::Arc;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;

use common::*;
use decoder::ProgramDecoder;

// use pyth_sdk_solana::PythError;
// use pyth_sdk_solana::state::{AccountType, GenericPriceAccount, MAGIC, SolanaPriceAccount, VERSION_2};

pub struct ArbiterClient {
  pub signer: Keypair,
  pub rpc: RpcClient,
  pub decoder: Arc<ProgramDecoder>,
}

impl ArbiterClient {
  pub async fn new(signer: Keypair, rpc_url: String) -> anyhow::Result<Self> {
    Ok(Self {
      signer,
      rpc: RpcClient::new(rpc_url),
      decoder: Arc::new(ProgramDecoder::new()?),
    })
  }

  pub fn rpc(&self) -> &RpcClient {
    &self.rpc
  }

  pub fn read_keypair_from_env(env_key: &str) -> anyhow::Result<Keypair> {
    read_keypair_from_env(env_key)
  }
}