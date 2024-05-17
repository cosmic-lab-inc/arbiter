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
use nexus::{DriftClient, HistoricalPerformance, HistoricalSettlePnl, MarketInfo, Nexus, OraclePrice, RemainingAccountMaps, RemainingAccountParams};
use nexus::drift_cpi::*;
use crate::Time;
use nexus::drift_cpi::drift;
use nexus::drift_cpi::ix_accounts::PlacePerpOrder;
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
use solana_sdk::transaction::Transaction;
use solana_transaction_status::UiTransactionEncoding;
use tokio::sync::RwLockReadGuard;
use crate::cache::AccountCache;
use crate::types::ChannelEvent;
use tokio::sync::RwLock;

pub struct Arbiter {
  signer: Keypair,
  pub nexus: Arc<Nexus>,
  pub cache: Arc<RwLock<AccountCache>>
}

impl Arbiter {
  pub async fn new(signer: Keypair, rpc: &str, geyser_ws: &str, pubsub_ws: &str) -> anyhow::Result<Self> {
    Ok(Self {
      signer,
      nexus: Arc::new(Nexus::new(rpc, geyser_ws, pubsub_ws).await?),
      cache: Arc::new(RwLock::new(AccountCache::new()))
    })
  }

  /// Assumes .env contains key "WALLET" with keypair byte array. Example: `WALLET=[1,2,3,4,5]`
  /// Assumes .env contains key "RPC_URL" with HTTP endpoint.
  /// Assumes .env contains key "PUBSUB_WS_URL" with WSS endpoint.
  /// Assumes .env contains key "GEYSER_WS_URL" with WSS endpoint.
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
  pub async fn stream_accounts(&self) -> anyhow::Result<()> {
    self.cache.write().await.load_all(self.rpc()).await?;

    // accounts to subscribe to
    let perps = DriftClient::perp_markets(self.rpc()).await?;
    let spots = DriftClient::spot_markets(self.rpc()).await?;
    let perp_markets: Vec<Pubkey> = perps.iter().map(|p| p.key).collect();
    let spot_markets: Vec<Pubkey> = spots.iter().map(|s| s.key).collect();
    let user = DriftClient::user_pda(&self.signer.pubkey(), 0)?;
    let users = [user, solana_sdk::pubkey!("H5jfagEnMVNH3PMc2TU2F7tNuXE6b4zCwoL5ip1b4ZHi")];

    //
    // Drift program account updates
    //
    let keys = perp_markets.iter().chain(spot_markets.iter()).chain(users.iter()).cloned().collect::<Vec<Pubkey>>();
    let config = RpcProgramAccountsConfig {
      // filters: Some(vec![
      //   RpcFilterType::Memcmp(Memcmp::new_base58_encoded(0, &User::discriminator())),
      //   RpcFilterType::Memcmp(Memcmp::new_base58_encoded(0, &PerpMarket::discriminator())),
      //   RpcFilterType::Memcmp(Memcmp::new_base58_encoded(0, &SpotMarket::discriminator()))
      // ]),
      account_config: RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64),
        commitment: Some(CommitmentConfig::processed()),
        ..Default::default()
      },
      ..Default::default()
    };
    let nexus = self.nexus.clone();
    let cache = self.cache.clone();
    tokio::task::spawn(async move {
      let (mut stream, _unsub) = nexus.stream_program(&id(), Some(config)).await?;
      while let Some(event) = stream.next().await {
        let RpcKeyedAccount {
          pubkey,
          account
        } = event.value;
        let key = Pubkey::from_str(&pubkey)?;

        if keys.contains(&key) {
          if let UiAccountData::Binary(data, UiAccountEncoding::Base64) = account.data {
            let data = general_purpose::STANDARD.decode(data)?;
            let acct = AccountType::decode(&data[..]).map_err(
              |e| anyhow::anyhow!("Failed to decode account: {:?}", e)
            )?;
            match acct {
              AccountType::PerpMarket(decoded) => {
                if perp_markets.contains(&key) {
                  println!("PerpMarket, {}", DriftClient::decode_name(&decoded.name));
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
                  println!("SpotMarket, {}", DriftClient::decode_name(&decoded.name));
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
                  println!("User, {}", DriftClient::decode_name(&decoded.name));
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
          }
        }
      }

      Result::<_, anyhow::Error>::Ok(())
    });

    //
    // Pyth program account updates
    //
    let perp_oracles: Vec<Pubkey> = perps.iter().map(|p| p.decoded.amm.oracle).collect();
    let spot_oracles: Vec<Pubkey> = spots.iter().map(|s| s.decoded.oracle).collect();
    let keys = perp_oracles.iter().chain(spot_oracles.iter()).cloned().collect::<Vec<Pubkey>>();
    let config = RpcProgramAccountsConfig {
      account_config: RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64),
        commitment: Some(CommitmentConfig::processed()),
        ..Default::default()
      },
      ..Default::default()
    };
    let nexus = self.nexus.clone();
    let cache = self.cache.clone();
    tokio::task::spawn(async move {
      let (mut stream, _unsub) = nexus.stream_program(&nexus::PYTH_PROGRAM_ID, Some(config)).await?;
      while let Some(event) = stream.next().await {
        let RpcKeyedAccount {
          pubkey,
          account
        } = event.value;
        let key = Pubkey::from_str(&pubkey)?;

        if keys.contains(&key) {
          if let UiAccountData::Binary(data, UiAccountEncoding::Base64) = &account.data {
            let account = Account {
              lamports: account.lamports,
              data: general_purpose::STANDARD.decode(&data[..])?,
              owner: Pubkey::from_str(&account.owner)?,
              executable: account.executable,
              rent_epoch: account.rent_epoch
            };

            if perp_oracles.contains(&key) {
              let read_cache = cache.read().await;
              let decoded = read_cache.perp_oracle(&key)?.decoded.clone();
              drop(read_cache);
              let mut cache = cache.write().await;
              println!("PerpOracle, market: {}, src: {:?}", DriftClient::decode_name(&decoded.market.name), decoded.source);
              cache.perp_oracles.insert(key, DecodedAccountContext {
                key,
                account: account.clone(),
                slot: event.context.slot,
                decoded
              });
            } else if spot_oracles.contains(&key) {
              let read_cache = cache.read().await;
              let decoded = read_cache.spot_oracle(&key)?.decoded.clone();
              drop(read_cache);
              let mut cache = cache.write().await;
              println!("SpotOracle, market: {}, src: {:?}", DriftClient::decode_name(&decoded.market.name), decoded.source);
              cache.spot_oracles.insert(key, DecodedAccountContext {
                key,
                account,
                slot: event.context.slot,
                decoded
              });
            }
          }
        }
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

  fn account_info<'a>(key: &'a Pubkey, signs: bool, writable: bool, exec: bool, acct: &'a mut Account) -> AccountInfo<'a> {
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

  pub async fn add_spot_market_to_remaining_accounts_map(
    &self,
    market_index: u16,
    writable: bool,
    oracle_account_map: &mut HashMap<String, AccountInfo<'static>>,
    spot_market_account_map: &mut HashMap<u16, AccountInfo<'static>>,
  ) -> anyhow::Result<()> {
    let spot_market_key = DriftClient::spot_market_pda(market_index)?;
    let spot_market = self.cache.read().await.spot_market(&spot_market_key)?.clone();
    let oracle = self.cache.read().await.spot_oracle(&spot_market.decoded.oracle)?.clone();
    let spot_market_acct = spot_market.account;

    let acct_info = Self::account_info(
      Box::leak(Box::new(spot_market_key)),
      false,
      writable,
      false,
      Box::leak(Box::new(spot_market_acct))
    );
    spot_market_account_map.insert(spot_market.decoded.market_index, acct_info);

    if spot_market.decoded.oracle != Pubkey::default() {
      let acct_info = Self::account_info(
        Box::leak(Box::new(spot_market.decoded.oracle)),
        false,
        false,
        false,
        Box::leak(Box::new(oracle.account))
      );
      oracle_account_map.insert(spot_market.decoded.oracle.to_string(), acct_info);
    }

    Ok(())
  }

  pub async fn add_perp_market_to_remaining_accounts_map(
    &self,
    market_index: u16,
    writable: bool,
    oracle_account_map: &mut HashMap<String, AccountInfo<'static>>,
    spot_market_account_map: &mut HashMap<u16, AccountInfo<'static>>,
    perp_market_account_map: &mut HashMap<u16, AccountInfo<'static>>
  ) -> anyhow::Result<()> {
    let perp_market_key = DriftClient::perp_market_pda(market_index)?;
    let perp_market = self.cache.read().await.perp_market(&perp_market_key)?.clone();
    let oracle = self.cache.read().await.perp_oracle(&perp_market.decoded.amm.oracle)?.clone();

    let acct_info = Self::account_info(
      Box::leak(Box::new(perp_market_key)),
      false,
      writable,
      false,
      Box::leak(Box::new(perp_market.account))
    );
    perp_market_account_map.insert(market_index, acct_info);

    let oracle_writable = matches!(perp_market.decoded.amm.oracle_source, OracleSource::Prelaunch) && writable;
    let oracle_acct_info = Self::account_info(
      Box::leak(Box::new(perp_market.decoded.amm.oracle)),
      false,
      oracle_writable,
      false,
      Box::leak(Box::new(oracle.account))
    );
    oracle_account_map.insert(perp_market.decoded.amm.oracle.to_string(), oracle_acct_info);

    self.add_spot_market_to_remaining_accounts_map(
      perp_market.decoded.quote_spot_market_index,
      false,
      oracle_account_map,
      spot_market_account_map
    ).await?;

    Ok(())
  }

  pub fn spot_position_available(pos: &SpotPosition) -> bool {
    pos.scaled_balance == 0 && pos.open_orders == 0
  }

  pub fn perp_position_available(pos: &PerpPosition) -> bool {
    pos.base_asset_amount == 0 && pos.open_orders == 0 && pos.quote_asset_amount == 0 && pos.lp_shares == 0
  }

  /// https://github.com/drift-labs/protocol-v2/blob/6808189602a5f255905018f769ca01bc0344a4bc/sdk/src/driftClient.ts#L1689
  pub async fn remaining_account_maps_for_users(
    &self,
    users: &[User]
  ) -> anyhow::Result<RemainingAccountMaps> {
    let mut oracle_account_map: HashMap<String, AccountInfo> = HashMap::new();
    let mut spot_market_account_map: HashMap<u16, AccountInfo> = HashMap::new();
    let mut perp_market_account_map: HashMap<u16, AccountInfo> = HashMap::new();

    for user in users.iter() {
      for spot_position in user.spot_positions {
        if Self::spot_position_available(&spot_position) {
          self.add_spot_market_to_remaining_accounts_map(
            spot_position.market_index,
            false,
            &mut oracle_account_map,
            &mut spot_market_account_map
          ).await?;

          if spot_position.open_asks != 0 || spot_position.open_bids != 0 {
            self.add_spot_market_to_remaining_accounts_map(
              QUOTE_SPOT_MARKET_INDEX,
              false,
              &mut oracle_account_map,
              &mut spot_market_account_map
            ).await?;
          }
        }
      }

      for perp_position in user.perp_positions {
        if !Self::perp_position_available(&perp_position) {
          self.add_perp_market_to_remaining_accounts_map(
            perp_position.market_index,
            false,
            &mut oracle_account_map,
            &mut spot_market_account_map,
            &mut perp_market_account_map
          ).await?;
        }
      }
    }

    Ok(RemainingAccountMaps {
      oracle_account_map,
      spot_market_account_map,
      perp_market_account_map
    })
  }

  /// https://github.com/drift-labs/protocol-v2/blob/6808189602a5f255905018f769ca01bc0344a4bc/sdk/src/driftClient.ts#L1519
  pub async fn remaining_accounts(
    &self,
    params: RemainingAccountParams
  ) -> anyhow::Result<Vec<AccountInfo<'static>>> {
    let RemainingAccountMaps {
      mut oracle_account_map,
      mut spot_market_account_map,
      mut perp_market_account_map
    } = self.remaining_account_maps_for_users(params.user_accounts.as_slice()).await?;

    let user_key = DriftClient::user_pda(&self.signer.pubkey(), 0)?;
    if params.use_market_last_slot_cache {
      let cache = self.cache.read().await;
      let last_user_slot = cache.user(&user_key)?.slot;
      for perp_market in cache.perp_markets() {
        // if cache has more recent slot than user positions account slot, add market to remaining accounts
        // otherwise remove from slot
        if perp_market.slot > last_user_slot {
          self.add_perp_market_to_remaining_accounts_map(
            perp_market.decoded.market_index,
            false,
            &mut oracle_account_map,
            &mut spot_market_account_map,
            &mut perp_market_account_map
          ).await?;
        }
      }

      for spot_market in cache.spot_markets() {
        // if cache has more recent slot than user positions account slot, add market to remaining accounts
        // otherwise remove from slot
        if spot_market.slot > last_user_slot {
          self.add_spot_market_to_remaining_accounts_map(
            spot_market.decoded.market_index,
            false,
            &mut oracle_account_map,
            &mut spot_market_account_map
          ).await?;
        }
      }
    }

    if let Some(readable_perp_market_indexes) = params.readable_perp_market_indexes {
      for market_index in readable_perp_market_indexes {
        self.add_perp_market_to_remaining_accounts_map(
          market_index,
          false,
          &mut oracle_account_map,
          &mut spot_market_account_map,
          &mut perp_market_account_map
        ).await?;
      }
    }
    // skipping mustIncludePerpMarketIndexes that typescript client does

    if let Some(readable_spot_market_indexes) = params.readable_spot_market_indexes {
      for market_index in readable_spot_market_indexes {
        self.add_spot_market_to_remaining_accounts_map(
          market_index,
          false,
          &mut oracle_account_map,
          &mut spot_market_account_map
        ).await?;
      }
    }
    // skipping mustIncludeSpotMarketIndexes that typescript client does

    if let Some(writable_perp_market_indexes) = params.writable_perp_market_indexes {
      for market_index in writable_perp_market_indexes {
        self.add_perp_market_to_remaining_accounts_map(
          market_index,
          true,
          &mut oracle_account_map,
          &mut spot_market_account_map,
          &mut perp_market_account_map
        ).await?;
      }
    }

    if let Some(writable_spot_market_indexes) = params.writable_spot_market_indexes {
      for market_index in writable_spot_market_indexes {
        self.add_spot_market_to_remaining_accounts_map(
          market_index,
          true,
          &mut oracle_account_map,
          &mut spot_market_account_map
        ).await?;
      }
    }

    let mut metas: Vec<AccountInfo<'static>> = vec![];
    metas.extend(oracle_account_map.into_values().collect::<Vec<_>>());
    metas.extend(spot_market_account_map.into_values().collect::<Vec<_>>());
    metas.extend(perp_market_account_map.into_values().collect::<Vec<_>>());

    Ok(metas)
  }

  /// https://github.com/drift-labs/protocol-v2/blob/6808189602a5f255905018f769ca01bc0344a4bc/sdk/src/driftClient.ts#L3377
  pub async fn place_orders_ix(&self, params: Vec<OrderParams>) -> anyhow::Result<()> {
    let signer_key = self.signer.pubkey();
    let state_key = DriftClient::state_pda();
    let user_key = DriftClient::user_pda(&self.signer.pubkey(), 0)?;
    let res = self.rpc().get_multiple_accounts_with_commitment(&[user_key, state_key, self.signer.pubkey()], CommitmentConfig::processed()).await?;
    let user_acct = res.value[0].clone().ok_or(anyhow::anyhow!("User account not found"))?;
    let state_acct = res.value[1].clone().ok_or(anyhow::anyhow!("State account not found"))?;
    let signer_acct = res.value[2].clone().ok_or(anyhow::anyhow!("Signer account not found"))?;
    // let user = Self::account_info(&user_key, false, true, false, &mut user_acct);
    // let state = Self::account_info(&state_key, false, false, false, &mut state_acct);
    // let signer = Self::account_info(&signer_key, true, true, false, &mut signer_acct);
    // let authority = anchor_lang::prelude::Signer::try_from(&signer)?;
    let user = Self::account_info(
      Box::leak(Box::new(user_key)),
      false,
      true,
      false,
      Box::leak(Box::new(user_acct))
    );
    let state = Self::account_info(
      Box::leak(Box::new(state_key)),
      false,
      false,
      false,
      Box::leak(Box::new(state_acct))
    );
    let signer = Self::account_info(
      Box::leak(Box::new(signer_key)),
      true,
      true,
      false,
      Box::leak(Box::new(signer_acct))
    );
    let authority = anchor_lang::prelude::Signer::try_from(Box::leak(Box::new(signer)))?;

    let mut perp_indexes = vec![];
    let mut spot_indexes = vec![];
    for param in params.iter() {
      match param.market_type {
        MarketType::Perp => perp_indexes.push(param.market_index),
        MarketType::Spot => spot_indexes.push(param.market_index)
      }
    }


    let user_acct = self.cache.read().await.user(&user_key)?.clone().decoded;
    let rem_accts = self.remaining_accounts(RemainingAccountParams {
      user_accounts: vec![user_acct],
      readable_perp_market_indexes: Some(perp_indexes),
      writable_perp_market_indexes: None,
      readable_spot_market_indexes: Some(spot_indexes),
      writable_spot_market_indexes: None,
      use_market_last_slot_cache: true
    }).await?;


    let program_id = id();

    let base_asset_amount = 0;
    let mut order_params: Vec<OrderParams> = vec![];
    for mut param in params {
      param.base_asset_amount = base_asset_amount;
      order_params.push(param);
    }
    let ix_data = instruction::PlaceOrders {
      _params: order_params
    };

    let ix_accts = ix_accounts::PlaceOrders {
      state,
      user,
      authority,
    };

    // let bumps = <ix_accounts::PlaceOrders as Bumps>::Bumps::default();
    // let ctx = Context::new(&drift_id, &mut ix_accts, rem_accts.as_slice(), bumps);
    // drift::place_orders(ctx, order_params.clone())?;

    let drift_acct = self.rpc().get_account(&program_id).await?;
    let data = Box::leak(Box::new(drift_acct.data));
    let program = AccountInfo::new(
      Box::leak(Box::new(program_id)),
      false,
      false,
      Box::leak(Box::new(drift_acct.lamports)),
      data.as_mut_slice(),
      Box::leak(Box::new(drift_acct.owner)),
      drift_acct.executable,
      drift_acct.rent_epoch
    );
    let ctx = CpiContext::new(program, ix_accts)
      .with_remaining_accounts(rem_accts);
    let accounts = ctx.to_account_metas(None);

    let mut ixs = vec![];

    let prior_fee = self.get_recent_priority_fee(program_id, None).await?;
    self.with_priority_fee(&mut ixs, prior_fee, None)?;

    ixs.push(Instruction {
      program_id,
      accounts,
      data: ix_data.data()
    });

    let tx = Transaction::new_signed_with_payer(
      &ixs,
      Some(&self.signer.pubkey()),
      &[&self.signer],
      self.rpc().get_latest_blockhash().await?
    );

    let config = RpcSimulateTransactionConfig {
      commitment: Some(CommitmentConfig::processed()),
      encoding: Some(UiTransactionEncoding::JsonParsed),
      accounts: Some(RpcSimulateTransactionAccountsConfig {
        encoding: Some(UiAccountEncoding::Base64),
        addresses: vec![]
      }),
      ..Default::default()
    };
    self.rpc().simulate_transaction_with_config(&tx, config).await?;

    Ok(())
  }

  /// Get recent priority fee
  ///
  /// - `window` # of slots to include in the fee calculation
  async fn get_recent_priority_fee(
    &self,
    key: Pubkey,
    window: Option<usize>,
  ) -> anyhow::Result<u64> {
    let response = self
      .rpc()
      .get_recent_prioritization_fees(&[key])
      .await?;
    let window = window.unwrap_or(50);
    let fees: Vec<u64> = response
      .iter()
      .take(window)
      .map(|x| x.prioritization_fee)
      .collect();
    Ok(fees.iter().sum::<u64>() / fees.len() as u64)
  }

  /// Set the priority fee of the tx
  ///
  /// `microlamports_per_cu` the price per unit of compute in µ-lamports
  pub fn with_priority_fee(&self, ixs: &mut Vec<Instruction>, microlamports_per_cu: u64, cu_limit: Option<u32>) -> anyhow::Result<()> {
    let cu_limit_ix = ComputeBudgetInstruction::set_compute_unit_price(microlamports_per_cu);
    ixs.insert(0, cu_limit_ix);
    if let Some(cu_limit) = cu_limit {
      let cu_price_ix = ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);
      ixs.insert(1, cu_price_ix);
    }
    Ok(())
  }
}
