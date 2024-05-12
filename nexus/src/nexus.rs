use anchor_lang::{Discriminator, Owner};
use solana_account_decoder::{UiAccount, UiAccountEncoding};
use solana_rpc_client_api::config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use solana_rpc_client_api::filter::{Memcmp, RpcFilterType};
use solana_rpc_client_api::response::RpcKeyedAccount;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use crate::types::*;
use crate::websocket::*;

pub struct Nexus {
  pub client: NexusClient,
  pub trx_unsub: Option<StreamUnsub>,
  pub acct_unsub: Option<StreamUnsub>
}

impl Nexus {
  pub async fn new(wss: &str) -> anyhow::Result<Self> {
    Ok(Self {
      client: NexusClient::new(wss).await?,
      trx_unsub: None,
      acct_unsub: None
    })
  }

  pub async fn transactions(&mut self, key: &Pubkey) -> anyhow::Result<EventStream<serde_json::Value>> {
    let config = RpcTransactionsConfig {
      filter: TransactionSubscribeFilter::standard(key),
      options: TransactionSubscribeOptions::standard()
    };
    let (stream, unsub) = self.client.transaction_subscribe(config).await?;
    self.trx_unsub = Some(unsub);

    Ok(stream)
  }

  pub async fn accounts(&mut self, key: &Pubkey) -> anyhow::Result<EventStream<UiAccount>> {
    let config = RpcAccountInfoConfig {
      encoding: Some(UiAccountEncoding::Base64),
      commitment: Some(CommitmentConfig::confirmed()),
      ..Default::default()
    };
    let (stream, unsub) = self.client.account_subscribe(key, Some(config)).await?;
    self.acct_unsub = Some(unsub);
    Ok(stream)
  }

  pub async fn program<A: Discriminator + Owner>(&mut self) -> anyhow::Result<EventStream<RpcKeyedAccount>> {
    let filter = RpcFilterType::Memcmp(Memcmp::new_base58_encoded(0, &A::discriminator()));
    let account_config = RpcAccountInfoConfig {
      encoding: Some(UiAccountEncoding::Base64),
      commitment: Some(CommitmentConfig::confirmed()),
      ..Default::default()
    };
    let config = RpcProgramAccountsConfig {
      filters: Some(vec![filter]),
      account_config,
      ..Default::default()
    };
    let (stream, unsub) = self.client.program_subscribe(&A::owner(), Some(config)).await?;
    self.acct_unsub = Some(unsub);
    Ok(stream)
  }
}