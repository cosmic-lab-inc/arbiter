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

use nexus::{AccountContext, Cache, DecodedAccountContext, DriftClient, DriftUtils, MarketId, Nexus, OraclePrice, read_keypair_from_env, to_account, trunc, TrxBuilder};
use nexus::drift_cpi::{AccountType, BASE_PRECISION, Decode, OrderParams, PositionDirection, PRICE_PRECISION};

pub struct Arbiter {
  pub signer: Arc<Keypair>,
  pub rpc: Arc<RpcClient>,
  pub nexus: Arc<Nexus>,
  pub cache: Arc<RwLock<Cache>>,
  pub drift: DriftClient,
}

impl Arbiter {
  pub async fn new(signer: Keypair, rpc_url: &str, api_key: &str, sub_account_id: u16) -> anyhow::Result<Self> {
    let signer = Arc::new(signer);
    log::info!("Arbiter using wallet: {}", signer.pubkey());
    let rpc = Arc::new(RpcClient::new_with_timeout(rpc_url.to_string(), Duration::from_secs(90)));
    Ok(Self {
      drift: DriftClient::new(signer.clone(), rpc.clone(), sub_account_id).await?,
      rpc,
      signer,
      nexus: Arc::new(Nexus::new(rpc_url, api_key).await?),
      cache: Arc::new(RwLock::new(Cache::new())),
    })
  }

  /// Assumes .env contains key "WALLET" with keypair byte array. Example: `WALLET=[1,2,3,4,5]`
  /// Assumes .env contains key "RPC_URL" with HTTP endpoint.
  /// Assumes .env contains key "WS_URL" with WSS endpoint.
  pub async fn new_from_env(sub_account_id: u16) -> anyhow::Result<Self> {
    let signer = read_keypair_from_env("WALLET")?;
    let rpc = std::env::var("RPC_URL")?;
    let api_key = std::env::var("API_KEY")?;
    Self::new(signer, &rpc, &api_key, sub_account_id).await
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
    let perp_markets: Arc<Vec<Pubkey>> = Arc::new(perps.iter().map(|p| p.key).collect());
    let spot_markets: Arc<Vec<Pubkey>> = Arc::new(spots.iter().map(|s| s.key).collect());
    let user = DriftUtils::user_pda(&self.signer.pubkey(), 0);
    let users = [user];
    let perp_oracles: Arc<Vec<Pubkey>> = Arc::new(perps.iter().map(|p| p.decoded.amm.oracle).collect());
    let spot_oracles: Arc<Vec<Pubkey>> = Arc::new(spots.iter().map(|s| s.decoded.oracle).collect());
    let accounts = Arc::new(vec![
      nexus::drift_cpi::id(),
      solana_sdk::system_program::id(),
      solana_sdk::rent::Rent::id(),
      self.signer.pubkey()
    ]);

    let auths = [self.signer.pubkey()];
    self.cache.write().await.load_all(self.rpc(), &users, &accounts, &auths).await?;

    let keys = perp_markets.iter()
                           .chain(spot_markets.iter())
                           .chain(users.iter())
                           .chain(perp_oracles.iter())
                           .chain(spot_oracles.iter())
                           .chain(accounts.iter())
                           .cloned().collect::<Vec<Pubkey>>();
    for key in keys {
      let nexus = self.nexus.clone();
      let cache = self.cache.clone();
      let perp_markets = perp_markets.clone();
      let spot_markets = spot_markets.clone();
      let perp_oracles = perp_oracles.clone();
      let spot_oracles = spot_oracles.clone();
      let accounts = accounts.clone();
      tokio::task::spawn(async move {
        let (mut stream, _unsub) = nexus.stream_account(&key).await?;
        while let Some(event) = stream.next().await {
          let account = event.value;

          if let UiAccountData::Binary(data, UiAccountEncoding::Base64) = &account.data {
            let data = general_purpose::STANDARD.decode(data)?;

            //
            // Drift program account updates
            //
            if Pubkey::from_str(&account.owner)? == nexus::drift_cpi::id() {
              let acct = AccountType::decode(&data[..]).map_err(
                |e| anyhow::anyhow!("Failed to decode account: {:?}", e)
              )?;
              match acct {
                AccountType::PerpMarket(decoded) => {
                  if perp_markets.contains(&key) {
                    log::debug!("PerpMarket, {}", DriftUtils::decode_name(&decoded.name));
                    let mut cache = cache.write().await;
                    let account = Account {
                      lamports: account.lamports,
                      data,
                      owner: Pubkey::from_str(&account.owner)?,
                      executable: account.executable,
                      rent_epoch: account.rent_epoch
                    };
                    cache.perp_markets.insert(key, DecodedAccountContext {
                      key,
                      account,
                      decoded,
                      slot: event.context.slot
                    });
                  }
                }
                AccountType::SpotMarket(decoded) => {
                  if spot_markets.contains(&key) {
                    log::debug!("SpotMarket, {}", DriftUtils::decode_name(&decoded.name));
                    let mut cache = cache.write().await;
                    let account = Account {
                      lamports: account.lamports,
                      data,
                      owner: Pubkey::from_str(&account.owner)?,
                      executable: account.executable,
                      rent_epoch: account.rent_epoch
                    };
                    cache.spot_markets.insert(key, DecodedAccountContext {
                      key,
                      account,
                      slot: event.context.slot,
                      decoded,
                    });
                  }
                }
                AccountType::User(decoded) => {
                  if users.contains(&key) {
                    log::debug!("User, {}", DriftUtils::decode_name(&decoded.name));
                    let mut cache = cache.write().await;
                    let account = Account {
                      lamports: account.lamports,
                      data,
                      owner: Pubkey::from_str(&account.owner)?,
                      executable: account.executable,
                      rent_epoch: account.rent_epoch
                    };
                    cache.users.insert(key, DecodedAccountContext {
                      key,
                      account,
                      slot: event.context.slot,
                      decoded,
                    });
                  }
                }
                _ => {}
              }
            } else if perp_oracles.contains(&key) {
              // perp oracle account
              let account = to_account(account)?;

              let read_cache = cache.read().await;
              let decoded = read_cache.find_perp_oracle(&key)?.decoded.clone();
              drop(read_cache);
              let mut cache = cache.write().await;
              log::debug!("PerpOracle, market: {}, src: {:?}", DriftUtils::decode_name(&decoded.market.name), decoded
                .source);
              cache.perp_oracles.insert(key, DecodedAccountContext {
                key,
                account: account.clone(),
                slot: event.context.slot,
                decoded
              });
            } else if spot_oracles.contains(&key) {
              // spot oracle account
              let account = to_account(account)?;

              let read_cache = cache.read().await;
              let decoded = read_cache.find_spot_oracle(&key)?.decoded.clone();
              drop(read_cache);
              let mut cache = cache.write().await;
              log::debug!("SpotOracle, market: {}, src: {:?}", DriftUtils::decode_name(&decoded.market.name), decoded
                .source);
              cache.spot_oracles.insert(key, DecodedAccountContext {
                key,
                account,
                slot: event.context.slot,
                decoded
              });
            } else if accounts.contains(&key) {
              // program updates
              let account = to_account(account)?;
              let mut cache = cache.write().await;
              log::debug!("Account, {}", &key);
              cache.accounts.insert(key, AccountContext {
                key,
                account,
                slot: event.context.slot,
              });
            }
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

  pub async fn place_orders(&self, orders: Vec<OrderParams>, market_filter: Option<&[MarketId]>) -> anyhow::Result<()> {
    let mut trx = self.drift.new_tx(true);
    self.drift.place_orders_ix(&self.cache, orders, market_filter, &mut trx).await?;
    if !trx.ixs().is_empty() {
      trx.simulate(&self.signer, &vec![self.signer.deref()], nexus::drift_cpi::id()).await?;
    }
    Ok(())
  }
}
