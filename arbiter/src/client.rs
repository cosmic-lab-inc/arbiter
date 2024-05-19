#![allow(unused_imports)]

use std::collections::{BTreeSet, HashMap};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use anchor_lang::prelude::{AccountInfo, AccountMeta, CpiContext};
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
use crate::Time;
use anchor_lang::{Accounts, Bumps, Discriminator, InstructionData, ToAccountMetas};
use anchor_lang::context::Context;
use base64::Engine;
use base64::engine::general_purpose;
use crossbeam::channel::{Receiver, Sender};
use futures::StreamExt;
use solana_account_decoder::{UiAccount, UiAccountData, UiAccountEncoding};
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig, RpcSimulateTransactionAccountsConfig, RpcSimulateTransactionConfig};
use solana_client::rpc_filter::{Memcmp, RpcFilterType};
use solana_client::rpc_response::RpcKeyedAccount;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::instruction::Instruction;
use solana_sdk::sysvar::SysvarId;
use solana_sdk::transaction::Transaction;
use solana_transaction_status::UiTransactionEncoding;
use tokio::sync::RwLockReadGuard;
use nexus::{AccountCache, DriftClient, Nexus, OraclePrice, to_account};
use tokio::sync::RwLock;
use nexus::drift_cpi::{AccountType, Decode, OrderParams, PositionDirection, BASE_PRECISION, PRICE_PRECISION};

pub struct Arbiter {
  pub signer: Keypair,
  pub nexus: Arc<Nexus>,
  pub cache: Arc<RwLock<AccountCache>>
}

impl Arbiter {
  pub async fn new(signer: Keypair, rpc: &str, geyser_ws: &str) -> anyhow::Result<Self> {
    Ok(Self {
      signer,
      nexus: Arc::new(Nexus::new(rpc, geyser_ws).await?),
      cache: Arc::new(RwLock::new(AccountCache::new()))
    })
  }

  /// Assumes .env contains key "WALLET" with keypair byte array. Example: `WALLET=[1,2,3,4,5]`
  /// Assumes .env contains key "RPC_URL" with HTTP endpoint.
  /// Assumes .env contains key "WS_URL" with WSS endpoint.
  pub async fn new_from_env() -> anyhow::Result<Self> {
    Ok(Self {
      signer: read_keypair_from_env("WALLET")?,
      nexus: Arc::new(Nexus::new_from_env().await?),
      cache: Arc::new(RwLock::new(AccountCache::new()))
    })
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

  /// Subscribe to all perp/spot markets and our user account from the Drift program,
  /// as well as the Pyth oracles for the perp/spot markets.
  pub async fn subscribe(&self) -> anyhow::Result<()> {

    // accounts to subscribe to
    let perps = DriftClient::perp_markets(self.rpc()).await?;
    let spots = DriftClient::spot_markets(self.rpc()).await?;
    let perp_markets: Arc<Vec<Pubkey>> = Arc::new(perps.iter().map(|p| p.key).collect());
    let spot_markets: Arc<Vec<Pubkey>> = Arc::new(spots.iter().map(|s| s.key).collect());
    let user = DriftClient::user_pda(&self.signer.pubkey(), 0)?;
    let users = [user];
    let perp_oracles: Arc<Vec<Pubkey>> = Arc::new(perps.iter().map(|p| p.decoded.amm.oracle).collect());
    let spot_oracles: Arc<Vec<Pubkey>> = Arc::new(spots.iter().map(|s| s.decoded.oracle).collect());
    let programs = Arc::new(vec![
      nexus::drift_cpi::id(),
      solana_sdk::system_program::id(),
      solana_sdk::rent::Rent::id()
    ]);

    let auths = [self.signer.pubkey()];
    self.cache.write().await.load_all(self.rpc(), &users, &programs, &auths).await?;

    let keys = perp_markets.iter()
                           .chain(spot_markets.iter())
                           .chain(users.iter())
                           .chain(perp_oracles.iter())
                           .chain(spot_oracles.iter())
                           .chain(programs.iter())
                           .cloned().collect::<Vec<Pubkey>>();
    for key in keys {
      let nexus = self.nexus.clone();
      let cache = self.cache.clone();
      let perp_markets = perp_markets.clone();
      let spot_markets = spot_markets.clone();
      let perp_oracles = perp_oracles.clone();
      let spot_oracles = spot_oracles.clone();
      let programs = programs.clone();
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
                    log::debug!("PerpMarket, {}", DriftClient::decode_name(&decoded.name));
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
                    log::debug!("SpotMarket, {}", DriftClient::decode_name(&decoded.name));
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
                    log::debug!("User, {}", DriftClient::decode_name(&decoded.name));
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
              let account = to_account(account, data)?;

              let read_cache = cache.read().await;
              let decoded = read_cache.find_perp_oracle(&key)?.decoded.clone();
              drop(read_cache);
              let mut cache = cache.write().await;
              log::debug!("PerpOracle, market: {}, src: {:?}", DriftClient::decode_name(&decoded.market.name), decoded.source);
              cache.perp_oracles.insert(key, DecodedAccountContext {
                key,
                account: account.clone(),
                slot: event.context.slot,
                decoded
              });
            } else if spot_oracles.contains(&key) {
              // spot oracle account
              let account = to_account(account, data)?;

              let read_cache = cache.read().await;
              let decoded = read_cache.find_spot_oracle(&key)?.decoded.clone();
              drop(read_cache);
              let mut cache = cache.write().await;
              log::debug!("SpotOracle, market: {}, src: {:?}", DriftClient::decode_name(&decoded.market.name), decoded.source);
              cache.spot_oracles.insert(key, DecodedAccountContext {
                key,
                account,
                slot: event.context.slot,
                decoded
              });
            } else if programs.contains(&key) {
              // program updates
              let account = to_account(account, data)?;
              let mut cache = cache.write().await;
              log::debug!("Program, {}", &key);
              cache.programs.insert(key, AccountContext {
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
    println!(
      "{}, {} {} {} @ {} as {:?}",
      name,
      dir,
      base,
      oracle_price.name,
      limit_price,
      params.order_type
    );
  }
}
