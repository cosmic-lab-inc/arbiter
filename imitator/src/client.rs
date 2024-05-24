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
use solana_sdk::sysvar::SysvarId;
use solana_sdk::transaction::Transaction;
use solana_transaction_status::UiTransactionEncoding;
use tokio::io::ReadBuf;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;

use nexus::{AcctCtx, Cache, DecodedAcctCtx, DriftClient, DriftUtils, MarketId, Nexus, OraclePrice, read_keypair_from_env, StreamEvent, StreamUnsub, ToAccount, TransactionNotification, trunc, TrxBuilder};
use nexus::drift_cpi::{AccountType, BASE_PRECISION, Decode, OrderParams, PositionDirection, PRICE_PRECISION, QUOTE_SPOT_MARKET_MINT};

pub struct Imitator {
  pub signer: Arc<Keypair>,
  pub rpc: Arc<RpcClient>,
  pub nexus: Arc<Nexus>,
  pub cache: Arc<RwLock<Cache>>,
  pub drift: DriftClient,
  pub copy_user: Pubkey,
}

impl Imitator {
  pub async fn new(
    signer: Keypair,
    rpc_url: &str,
    api_key: &str,
    sub_account_id: u16,
    copy_user: Pubkey,
    cache_depth: Option<usize>,
  ) -> anyhow::Result<Self> {
    // 200 slots = 80 seconds of account cache
    let cache_depth = cache_depth.unwrap_or(200);
    let signer = Arc::new(signer);
    log::info!("Imitator using wallet: {}", signer.pubkey());
    let rpc = Arc::new(RpcClient::new_with_timeout(rpc_url.to_string(), Duration::from_secs(90)));
    Ok(Self {
      drift: DriftClient::new(signer.clone(), rpc.clone(), sub_account_id).await?,
      rpc,
      signer,
      nexus: Arc::new(Nexus::new(rpc_url, api_key).await?),
      cache: Arc::new(RwLock::new(Cache::new(cache_depth))),
      copy_user,
    })
  }

  /// Assumes .env contains key "WALLET" with keypair byte array. Example: `WALLET=[1,2,3,4,5]`
  /// Assumes .env contains key "RPC_URL" with HTTP endpoint.
  /// Assumes .env contains key "WS_URL" with WSS endpoint.
  pub async fn new_from_env(sub_account_id: u16, copy_user: Pubkey, cache_depth: Option<usize>) -> anyhow::Result<Self> {
    let signer = read_keypair_from_env("WALLET")?;
    let rpc = std::env::var("RPC_URL")?;
    let api_key = std::env::var("API_KEY")?;
    Self::new(signer, &rpc, &api_key, sub_account_id, copy_user, cache_depth).await
  }

  pub fn nexus(&self) -> Arc<Nexus> {
    self.nexus.clone()
  }

  pub fn rpc(&self) -> &RpcClient {
    &self.nexus.rpc
  }

  pub fn client(&self) -> &Client {
    &self.nexus.client
  }

  pub async fn monitor_transactions(&self) -> anyhow::Result<(StreamEvent<TransactionNotification>, StreamUnsub)> {
    self.nexus.stream_transactions(&self.copy_user).await
  }

  /// Initialize [`User`] and [`UserStats`] accounts,
  /// and deposit 100% of available USDC from the wallet.
  pub async fn setup(&self) -> anyhow::Result<()> {
    self.drift.setup_user().await?;
    Ok(())
  }

  /// Subscribe to account changes for Drift perp/spot markets and user account,
  /// the perp/spot Pyth oracles, and the Drift, System, and Rent programs.
  pub async fn subscribe(&self) -> anyhow::Result<()> {
    // accounts to subscribe to
    let perps = DriftUtils::perp_markets(self.rpc()).await?;
    let spots = DriftUtils::spot_markets(self.rpc()).await?;
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
    let accounts = vec![
      nexus::drift_cpi::id(),
      solana_sdk::system_program::id(),
      solana_sdk::rent::Rent::id(),
      usdc_token_acct,
    ];

    let auths = [self.signer.pubkey()];
    self.cache.write().await.load_all(self.rpc(), &users, &accounts, &auths).await?;

    let keys = perp_markets.iter().chain(spot_markets.iter()).chain(users.iter()).chain(perp_oracles.iter()).chain(spot_oracles.iter()).chain(accounts.iter()).cloned().collect::<Vec<Pubkey>>();
    for key in keys {
      let nexus = self.nexus.clone();
      let cache = self.cache.clone();
      tokio::task::spawn(async move {
        let (mut stream, _unsub) = nexus.stream_account(&key).await?;
        while let Some(event) = stream.next().await {
          let account = event.value;
          if let UiAccountData::Binary(_, UiAccountEncoding::Base64) = &account.data {
            let account = account.to_account()?;
            cache.write().await.ring_mut(key).insert(event.context.slot, AcctCtx {
              key,
              account: account.clone(),
              slot: event.context.slot,
            });
          }
        }
        Result::<_, anyhow::Error>::Ok(())
      });
    }

    // slot subscription
    let nexus = self.nexus.clone();
    let cache = self.cache.clone();
    tokio::task::spawn(async move {
      let (mut stream, _unsub) = nexus.stream_slots().await?;
      while let Some(event) = stream.next().await {
        let mut cache = cache.write().await;
        cache.slot = event.slot;
      }
      Result::<_, anyhow::Error>::Ok(())
    });

    Ok(())
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
    log::info!(
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
    orders: Vec<OrderParams>,
    market_filter: Option<&[MarketId]>,
  ) -> anyhow::Result<()> {
    let mut trx = self.drift.new_tx(true);
    self.drift.copy_place_orders_ix(&self.cache, &self.copy_user, orders, market_filter, &mut trx).await?;
    if !trx.ixs().is_empty() {
      trx.simulate(&self.signer, &vec![self.signer.deref()], nexus::drift_cpi::id()).await?;
    }
    Ok(())
  }
}
