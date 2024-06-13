#![allow(unused_imports)]

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anchor_lang::context::Context;
use anchor_lang::prelude::{AccountInfo, AccountMeta, CpiContext};
use anchor_lang::{Accounts, Bumps, Discriminator, InstructionData, ToAccountMetas};
use base64::engine::general_purpose;
use base64::Engine;
use borsh::BorshDeserialize;
use crossbeam::channel::{Receiver, Sender};
use futures::StreamExt;
use log::{debug, info};
use reqwest::Client;
use solana_account_decoder::{UiAccount, UiAccountData, UiAccountEncoding};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::{
  RpcAccountInfoConfig, RpcProgramAccountsConfig, RpcSimulateTransactionAccountsConfig,
  RpcSimulateTransactionConfig,
};
use solana_client::rpc_filter::{Memcmp, RpcFilterType};
use solana_client::rpc_response::RpcKeyedAccount;
use solana_sdk::account::Account;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::slot_hashes::SlotHashes;
use solana_sdk::sysvar::SysvarId;
use solana_sdk::transaction::Transaction;
use solana_transaction_status::UiTransactionEncoding;
use tokio::io::ReadBuf;
use tokio::sync::RwLockReadGuard;
use tokio::sync::{RwLock, RwLockWriteGuard};
use yellowstone_grpc_proto::prelude::subscribe_request_filter_accounts_filter::Filter;
use yellowstone_grpc_proto::prelude::{
  subscribe_request_filter_accounts_filter_memcmp, CommitmentLevel, SubscribeRequestFilterAccounts,
  SubscribeRequestFilterAccountsFilter, SubscribeRequestFilterAccountsFilterMemcmp,
  SubscribeRequestFilterBlocks, SubscribeRequestFilterBlocksMeta, SubscribeRequestFilterSlots,
  SubscribeRequestFilterTransactions,
};

use crate::config::ImitatorConfig;
use nexus::drift_client::*;
use nexus::drift_cpi::{Decode, DiscrimToName, InstructionType, MarketType};
use nexus::*;

pub struct Imitator {
  read_only: bool,
  retry_until_confirmed: bool,
  pub signer: Arc<Keypair>,
  pub rpc: Arc<RpcClient>,
  pub drift: DriftClient,
  pub client: Arc<Client>,
  pub copy_user: Pubkey,
  pub market_filter: Option<Vec<MarketId>>,
  pub cache: Cache,
  rx: Receiver<TxStub>,
  leverage: f64,
}

impl Imitator {
  pub async fn new(
    sub_account_id: u16,
    copy_user: Pubkey,
    market_filter: Option<Vec<MarketId>>,
    cache_depth: Option<usize>,
  ) -> anyhow::Result<Self> {
    let ImitatorConfig {
      read_only,
      retry_until_confirmed,
      signer,
      rpc_url,
      grpc,
      x_token,
      leverage,
      ..
    } = ImitatorConfig::read()?;

    // 200 slots = 80 seconds of account cache
    let cache_depth = cache_depth.unwrap_or(200);
    let signer = Arc::new(signer);
    info!("Imitator using wallet: {}", signer.pubkey());
    let rpc = Arc::new(RpcClient::new_with_timeout(
      rpc_url,
      Duration::from_secs(90),
    ));
    let (tx, rx) = crossbeam::channel::unbounded::<TxStub>();

    let this = Self {
      read_only,
      retry_until_confirmed,
      drift: DriftClient::new(
        signer.clone(),
        rpc.clone(),
        sub_account_id,
        None,
        read_only,
        retry_until_confirmed,
      )
      .await?,
      rpc,
      signer,
      cache: Cache::new(cache_depth),
      client: Arc::new(Client::builder().timeout(Duration::from_secs(90)).build()?),
      copy_user,
      market_filter,
      rx,
      leverage,
    };

    let account_filter = this.account_filter().await?;
    let cfg = this.copy_trade_geyser_config(grpc, x_token, account_filter)?;
    // stream updates from gRPC
    let nexus = NexusClient::new(cfg)?;
    let cache = this.cache.clone();
    tokio::task::spawn(async move {
      nexus.stream(&cache, Some(tx), None, None).await?;
      Result::<_, anyhow::Error>::Ok(())
    });
    Ok(this)
  }

  pub fn rpc(&self) -> Arc<RpcClient> {
    self.rpc.clone()
  }
  pub async fn cache(&self) -> ReadCache {
    self.cache.read().await
  }
  pub fn client(&self) -> Arc<Client> {
    self.client.clone()
  }
  pub fn user(&self) -> &Pubkey {
    &self.drift.sub_account
  }

  fn copy_trade_geyser_config(
    &self,
    grpc: String,
    x_token: String,
    account_filter: Vec<Pubkey>,
  ) -> anyhow::Result<GeyserConfig> {
    Ok(GeyserConfig {
      grpc,
      x_token,
      slots: Some(SubscribeRequestFilterSlots {
        filter_by_commitment: Some(true),
      }),
      accounts: Some(SubscribeRequestFilterAccounts {
        account: account_filter.into_iter().map(|k| k.to_string()).collect(),
        owner: vec![],
        // subscribe to all Drift `User` accounts
        filters: vec![],
      }),
      transactions: Some(SubscribeRequestFilterTransactions {
        vote: Some(false),
        failed: Some(false),
        signature: None,
        account_include: vec![self.copy_user.to_string()],
        account_exclude: vec![],
        account_required: vec![],
      }),
      blocks_meta: None,
      commitment: CommitmentLevel::Processed,
    })
  }

  fn orderbook_geyser_config(&self, grpc: String, x_token: String) -> anyhow::Result<GeyserConfig> {
    let drift_program_id = id();
    Ok(GeyserConfig {
      grpc,
      x_token,
      slots: Some(SubscribeRequestFilterSlots {
        filter_by_commitment: Some(true),
      }),
      accounts: Some(SubscribeRequestFilterAccounts {
        account: vec![],
        owner: vec![drift_program_id.to_string(), PYTH_PROGRAM_ID.to_string()],
        filters: vec![],
      }),
      transactions: None,
      blocks_meta: None,
      commitment: CommitmentLevel::Processed,
    })
  }

  pub async fn start(&mut self) -> anyhow::Result<()> {
    self.drift.setup_user().await?;
    self.reset_orders().await?;

    while let Ok(tx) = self.rx.recv() {
      let mut trx = self.new_tx();
      for ix in tx.ixs {
        if ix.program == id() {
          let decoded_ix = InstructionType::decode(&ix.data[..])
            .map_err(|e| anyhow::anyhow!("Failed to decode instruction: {:?}", e))?;
          let name = InstructionType::discrim_to_name(ix.data[..8].try_into()?)
            .map_err(|e| anyhow::anyhow!("Failed to decode discrim to name: {:?}", e))?;
          info!("{}", name);

          match decoded_ix {
            InstructionType::PlacePerpOrder(ix) => {
              let params = ix._params;
              if self.allow_market(MarketId {
                index: params.market_index,
                kind: params.market_type,
              }) {
                self.place_orders(tx.slot, vec![params], &mut trx).await?;
                let price = self.drift.order_price(
                  MarketId {
                    index: params.market_index,
                    kind: params.market_type,
                  },
                  &self.cache().await,
                  Some(tx.slot),
                  &params,
                )?;
                DriftUtils::log_order(&params, &price, Some("PlacePerpOrder"));
              }
            }
            InstructionType::PlaceOrders(ix) => {
              let mut orders = vec![];
              for params in ix._params.iter() {
                if self.allow_market(MarketId {
                  index: params.market_index,
                  kind: params.market_type,
                }) {
                  orders.push(*params);
                }
              }
              if !orders.is_empty() {
                self.place_orders(tx.slot, orders, &mut trx).await?;
              }
            }
            InstructionType::CancelOrders(ix) => {
              if let (Some(index), Some(kind)) = (ix._market_index, ix._market_type) {
                let market = MarketId::from((index, kind));
                if self.allow_market(market) {
                  self
                    .cancel_orders(Some(market), ix._direction, &mut trx)
                    .await?;
                  info!("CancelOrders");
                }
              };
            }
            InstructionType::PlaceAndTakePerpOrder(ix) => {
              let params = ix._params;
              if self.allow_market(MarketId {
                index: params.market_index,
                kind: params.market_type,
              }) {
                let price = self.drift.order_price(
                  MarketId {
                    index: params.market_index,
                    kind: params.market_type,
                  },
                  &self.cache().await,
                  Some(tx.slot),
                  &params,
                )?;
                DriftUtils::log_order(&params, &price, Some("PlaceAndTakePerpOrder"));
                info!("PlaceAndTakePerpOrder: {:#?}", params);
                info!("https://solana.fm/tx/{}", tx.signature);
              }
            }
            _ => {}
          }
        }
      }
      if !self.read_only {
        trx.send_tx(id(), None).await?;
      }
    }
    Ok(())
  }

  async fn reset_orders(&self) -> anyhow::Result<()> {
    let mut trx = self.new_tx();
    trx = trx.retry_until_confirmed();
    self.cancel_orders(None, None, &mut trx).await?;
    trx.send_tx(id(), None).await
  }

  fn allow_market(&self, market: MarketId) -> bool {
    match &self.market_filter {
      Some(filter) => filter.contains(&market),
      None => true,
    }
  }

  /// Stream these accounts from geyser for usage in the engine
  pub async fn account_filter(&self) -> anyhow::Result<Vec<Pubkey>> {
    // accounts to subscribe to
    let perps = DriftUtils::perp_markets(&self.rpc()).await?;
    let spots = DriftUtils::spot_markets(&self.rpc()).await?;
    let perp_markets: Vec<Pubkey> = perps.iter().map(|p| p.key).collect();
    let spot_markets: Vec<Pubkey> = spots.iter().map(|s| s.key).collect();
    let user = DriftUtils::user_pda(&self.signer.pubkey(), 0);
    let users = [user, self.copy_user];
    let perp_oracles: Vec<Pubkey> = perps.iter().map(|p| p.decoded.amm.oracle).collect();
    let spot_oracles: Vec<Pubkey> = spots.iter().map(|s| s.decoded.oracle).collect();
    let auths = [self.signer.pubkey()];
    self
      .cache
      .write()
      .await
      .load(&self.rpc(), &users, None, &auths)
      .await?;
    let keys = perp_markets
      .iter()
      .chain(spot_markets.iter())
      .chain(users.iter())
      .chain(perp_oracles.iter())
      .chain(spot_oracles.iter())
      .cloned()
      .collect::<Vec<Pubkey>>();
    Ok(keys)
  }

  pub fn new_tx(&self) -> KeypairTrx<'_> {
    self.drift.new_tx(true)
  }

  pub async fn place_orders(
    &self,
    tx_slot: u64,
    orders: Vec<OrderParams>,
    trx: &mut KeypairTrx<'_>,
  ) -> anyhow::Result<()> {
    let market_filter = self.market_filter.as_deref();
    self
      .drift
      .copy_place_orders_ix(tx_slot, &self.cache().await, orders, market_filter, trx)
      .await?;
    Ok(())
  }

  pub async fn cancel_orders(
    &self,
    market: Option<MarketId>,
    direction: Option<PositionDirection>,
    trx: &mut KeypairTrx<'_>,
  ) -> anyhow::Result<()> {
    info!("cancel orders...");
    let market_filter = self.market_filter.as_deref();
    if let (Some(filter), Some(market)) = (market_filter, market) {
      if !filter.contains(&market) {
        return Ok(());
      }
    }
    self
      .drift
      .cancel_orders_ix(&self.cache().await, market, direction, trx)
      .await?;
    Ok(())
  }
}
