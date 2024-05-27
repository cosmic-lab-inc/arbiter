#![allow(unused_imports)]

use std::collections::{BTreeSet, HashMap};
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anchor_lang::{Accounts, Bumps, Discriminator, InstructionData, ToAccountMetas};
use anchor_lang::context::Context;
use anchor_lang::prelude::{AccountInfo, AccountMeta, CpiContext};
use base64::Engine;
use base64::engine::general_purpose;
use borsh::BorshDeserialize;
use crossbeam::channel::{Receiver, Sender};
use futures::StreamExt;
use log::info;
use reqwest::Client;
use solana_account_decoder::{UiAccount, UiAccountData, UiAccountEncoding};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig, RpcSimulateTransactionAccountsConfig, RpcSimulateTransactionConfig};
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
use tokio::sync::{RwLock, RwLockWriteGuard};
use tokio::sync::RwLockReadGuard;
use yellowstone_grpc_proto::prelude::{CommitmentLevel, SubscribeRequestFilterAccounts, SubscribeRequestFilterBlocks, SubscribeRequestFilterBlocksMeta, SubscribeRequestFilterSlots, SubscribeRequestFilterTransactions};

use nexus::*;
use nexus::drift_cpi::{Decode, DiscrimToName, InstructionType, MarketType};

pub struct Imitator {
  pub signer: Arc<Keypair>,
  pub rpc: Arc<RpcClient>,
  pub drift: DriftClient,
  pub client: Arc<Client>,
  pub copy_user: Pubkey,
  pub market_filter: Option<Vec<MarketId>>,
  pub cache: Arc<RwLock<Cache>>,
  rx: Receiver<TxStub>,
}

impl Imitator {
  pub async fn new(
    sub_account_id: u16,
    copy_user: Pubkey,
    market_filter: Option<Vec<MarketId>>,
    cache_depth: Option<usize>,
  ) -> anyhow::Result<Self> {
    let signer = read_keypair_from_env("WALLET")?;
    let rpc_url = std::env::var("RPC_URL")?;
    let grpc = std::env::var("GRPC")?;
    let x_token = std::env::var("X_TOKEN")?;

    // 200 slots = 80 seconds of account cache
    let cache_depth = cache_depth.unwrap_or(200);
    let signer = Arc::new(signer);
    info!("Imitator using wallet: {}", signer.pubkey());
    let rpc = Arc::new(RpcClient::new_with_timeout(rpc_url, Duration::from_secs(90)));
    let (tx, rx) = crossbeam::channel::unbounded::<TxStub>();

    let this = Self {
      drift: DriftClient::new(signer.clone(), rpc.clone(), sub_account_id).await?,
      rpc,
      signer,
      cache: Arc::new(RwLock::new(Cache::new(cache_depth))),
      client: Arc::new(Client::builder().timeout(Duration::from_secs(90)).build()?),
      copy_user,
      market_filter,
      rx,
    };

    let account_filter: Vec<String> = this.account_filter().await?.into_iter().map(|k| k.to_string()).collect();
    let cfg = GeyserConfig {
      grpc,
      x_token,
      slots: Some(SubscribeRequestFilterSlots {
        filter_by_commitment: Some(true)
      }),
      // slots: None,
      accounts: Some(SubscribeRequestFilterAccounts {
        account: account_filter,
        owner: vec![],
        filters: vec![],
      }),
      transactions: Some(SubscribeRequestFilterTransactions {
        vote: Some(false),
        failed: Some(false),
        signature: None,
        account_include: vec![copy_user.to_string()],
        account_exclude: vec![],
        account_required: vec![],
      }),
      blocks_meta: Some(SubscribeRequestFilterBlocksMeta {}),
      // blocks_meta: None,
      commitment: CommitmentLevel::Processed,
    };
    // stream updates from gRPC
    let nexus = NexusClient::new(cfg)?;
    let cache = this.cache.clone();
    tokio::task::spawn(async move {
      nexus.stream(&cache, tx).await?;
      Result::<_, anyhow::Error>::Ok(())
    });
    Ok(this)
  }

  pub fn rpc(&self) -> Arc<RpcClient> {
    self.rpc.clone()
  }
  pub fn cache(&self) -> &RwLock<Cache> { &self.cache }
  pub fn client(&self) -> Arc<Client> { self.client.clone() }

  /// 1. Initialize [`User`] and [`UserStats`] accounts,
  /// and deposit 100% of available USDC from the wallet.
  ///
  /// 2. Start geyser stream of account, transaction, and slot updates.
  ///
  /// 3. Listen to geyser stream of transactions.
  pub async fn start(&mut self) -> anyhow::Result<()> {
    self.drift.setup_user().await?;

    while let Ok(tx) = self.rx.recv() {
      // todo: bundle ix into same tx just, to copy at the tx level
      for ix in tx.ixs {
        if ix.program == id() {
          let decoded_ix = InstructionType::decode(&ix.data[..]).map_err(
            |e| anyhow::anyhow!("Failed to decode instruction: {:?}", e)
          )?;
          let discrim: [u8; 8] = ix.data[..8].try_into()?;
          let name = InstructionType::discrim_to_name(discrim).map_err(|e| anyhow::anyhow!("Failed to get ix discrim: {:?}", e))?;

          match decoded_ix {
            InstructionType::PlacePerpOrder(ix) => {
              let params = ix._params;
              let market_info = DriftUtils::perp_market_info(self.cache(), params.market_index).await?;
              if self.allow_market(MarketId {
                index: params.market_index,
                kind: params.market_type,
              }) {
                info!("{}", name);
                self.log_order(&name, &params, &market_info);
                self.place_orders(tx.slot, vec![params]).await?;
              }
            }
            InstructionType::PlaceAndTakePerpOrder(_ix) => {
              // todo? taker orders specify a maker which is unique to the trade, and therefore not copyable... I think.
              // let params = ix._params;
              // let market_info = DriftUtils::perp_market_info(self.cache(), params.market_index).await?;
              // if self.allow_market(MarketId {
              //   index: params.market_index,
              //   kind: params.market_type,
              // }) {
              // info!("{}", name);
              // self.log_order(&name, &params, &market_info);
              // }
            }
            InstructionType::PlaceOrders(ix) => {
              let mut orders = vec![];
              for params in ix._params.iter() {
                let market_info = DriftUtils::perp_market_info(self.cache(), params.market_index).await?;
                if self.allow_market(MarketId {
                  index: params.market_index,
                  kind: params.market_type,
                }) {
                  self.log_order(&name, params, &market_info);
                  orders.push(*params);
                }
              }
              self.place_orders(tx.slot, orders).await?;
            }
            InstructionType::CancelOrders(ix) => {
              let market = match (ix._market_index, ix._market_type) {
                (Some(index), Some(kind)) => {
                  Some(MarketId {
                    index,
                    kind,
                  })
                }
                (None, None) => None,
                _ => return Err(anyhow::anyhow!("Invalid market index and kind Options")),
              };
              self.cancel_orders(market, ix._direction).await?;
            }
            _ => {}
          }
        }
      }
    }
    Ok(())
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
    let usdc_token_acct = spl_associated_token_account::get_associated_token_address(
      &self.signer.pubkey(),
      &QUOTE_SPOT_MARKET_MINT,
    );
    let accounts = [
      id(),
      solana_sdk::system_program::id(),
      solana_sdk::rent::Rent::id(),
      usdc_token_acct,
    ];
    let auths = [self.signer.pubkey()];
    let now = std::time::Instant::now();
    self.cache.write().await.load_all(&self.rpc(), &users, &accounts, &auths).await?;
    log::debug!("time to load cache: {:?}", now.elapsed());
    let keys = perp_markets.iter().chain(spot_markets.iter()).chain(users.iter()).chain(perp_oracles.iter()).chain(spot_oracles.iter()).chain(accounts.iter()).cloned().collect::<Vec<Pubkey>>();
    assert!(keys.contains(&solana_sdk::pubkey!("H6ARHf6YXhGYeQfUzQNGk6rDNnLBQKrenN712K4AQJEG")));
    Ok(keys)
  }

  pub fn log_order(&self, name: &str, params: &OrderParams, oracle_price: &OraclePrice) {
    let dir = match params.direction {
      PositionDirection::Long => "long",
      PositionDirection::Short => "short"
    };
    let oracle_price_offset = match params.oracle_price_offset {
      None => 0.0,
      Some(offset) => trunc!(offset as f64 / PRICE_PRECISION as f64, 2)
    };
    let base = trunc!(params.base_asset_amount as f64 / BASE_PRECISION as f64, 2);
    let limit_price = trunc!(oracle_price.price + oracle_price_offset, 2);
    log::debug!(
      "{}, {} {} {} @ {} as {:?}",
      name,
      dir,
      base,
      oracle_price.name,
      limit_price,
      params.order_type
    );
  }

  pub async fn place_orders(
    &self,
    tx_slot: u64,
    orders: Vec<OrderParams>,
  ) -> anyhow::Result<()> {
    let mut trx = self.drift.new_tx(true);
    let market_filter = self.market_filter.as_deref();
    self.drift.copy_place_orders_ix(tx_slot, self.cache(), orders, market_filter, &mut trx).await?;
    if !trx.is_empty() {
      trx.simulate(&self.signer, &vec![self.signer.deref()], id()).await?;
    }
    Ok(())
  }

  pub async fn cancel_orders(
    &self,
    market: Option<MarketId>,
    direction: Option<PositionDirection>,
  ) -> anyhow::Result<()> {
    let mut trx = self.drift.new_tx(true);
    let market_filter = self.market_filter.as_deref();
    self.drift.cancel_orders_ix(self.cache(), market_filter, market, direction, &mut trx).await?;
    if !trx.is_empty() {
      trx.simulate(&self.signer, &vec![self.signer.deref()], id()).await?;
    }
    Ok(())
  }
}
