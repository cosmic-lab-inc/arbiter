use std::collections::HashMap;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;

use anchor_lang::{AccountDeserialize, Discriminator, InstructionData, ToAccountMetas};
use borsh::BorshDeserialize;
use log::{debug, info};
use rayon::prelude::*;
use solana_account_decoder::UiAccountEncoding;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use solana_client::rpc_filter::{Memcmp, RpcFilterType};
use solana_rpc_client_api::request::RpcRequest;
use solana_rpc_client_api::response::{OptionalContext, RpcKeyedAccount};
use solana_sdk::account::Account;
use solana_sdk::account_info::AccountInfo;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::native_token::LAMPORTS_PER_SOL;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::sysvar::SysvarId;
use tokio::sync::{RwLock, RwLockReadGuard};

use crate::{DecodedAcctCtx, Time, trunc};
use crate::*;
use crate::drift_client::trader::*;
use crate::drift_cpi::*;

pub struct DriftClient {
  /// either User account authority or delegate
  signer: Arc<Keypair>,
  rpc: Arc<RpcClient>,
  /// contextual on-chain program data
  program_data: ProgramData,
  /// the drift subaccount address
  sub_account: Pubkey,
}

impl DriftClient {
  /// Initialize a new [`TrxBuilder`] for default signer.

  pub async fn new(
    signer: Arc<Keypair>,
    rpc: Arc<RpcClient>,
    sub_account_id: u16,
  ) -> anyhow::Result<Self> {
    let (spot_markets, perp_markets) = DriftUtils::market_accounts(&rpc).await?;
    let lut = rpc.get_account(&MARKET_LOOKUP_TABLE).await?;
    let lookup_table = crate::utils::deserialize_lookup_table(MARKET_LOOKUP_TABLE, &lut)?;
    let program_data = ProgramData::new(
      spot_markets,
      perp_markets,
      lookup_table,
    );
    let sub_account = DriftUtils::user_pda(&signer.pubkey(), sub_account_id);
    Ok(Self {
      signer,
      rpc,
      program_data,
      sub_account,
    })
  }

  pub fn new_tx(&self, with_lookup_tables: bool) -> TrxBuilder {
    let alt = if with_lookup_tables {
      vec![self.program_data.lookup_table.clone()]
    } else {
      vec![]
    };
    TrxBuilder::new(
      self.rpc.clone(),
      false,
      alt,
    )
  }

  pub async fn add_spot_market_to_remaining_accounts_map(
    &self,
    cache: &RwLockReadGuard<'_, Cache>,
    market_index: u16,
    writable: bool,
    oracle_account_map: &mut HashMap<String, AccountInfo<'static>>,
    spot_market_account_map: &mut HashMap<u16, AccountInfo<'static>>,
  ) -> anyhow::Result<()> {
    let spot_market_key = DriftUtils::spot_market_pda(market_index);
    let spot_market = cache.decoded_account::<SpotMarket>(&spot_market_key, None)?;
    let oracle = cache.account(&spot_market.decoded.oracle, None)?.clone();
    let spot_market_acct = spot_market.account;

    let acct_info = spot_market_acct.to_account_info(
      spot_market_key,
      false,
      writable,
      false,
    );
    spot_market_account_map.insert(spot_market.decoded.market_index, acct_info);

    if spot_market.decoded.oracle != Pubkey::default() {
      let acct_info = oracle.account.to_account_info(
        spot_market.decoded.oracle,
        false,
        false,
        false,
      );
      oracle_account_map.insert(spot_market.decoded.oracle.to_string(), acct_info);
    }

    Ok(())
  }

  pub async fn add_perp_market_to_remaining_accounts_map(
    &self,
    cache: &RwLockReadGuard<'_, Cache>,
    market_index: u16,
    writable: bool,
    oracle_account_map: &mut HashMap<String, AccountInfo<'static>>,
    spot_market_account_map: &mut HashMap<u16, AccountInfo<'static>>,
    perp_market_account_map: &mut HashMap<u16, AccountInfo<'static>>,
  ) -> anyhow::Result<()> {
    let perp_market_key = DriftUtils::perp_market_pda(market_index);
    let perp_market = cache.decoded_account::<PerpMarket>(&perp_market_key, None)?;
    let oracle = cache.account(&perp_market.decoded.amm.oracle, None)?.clone();

    let acct_info = perp_market.account.to_account_info(
      perp_market_key,
      false,
      writable,
      false,
    );
    perp_market_account_map.insert(market_index, acct_info);

    let oracle_writable = matches!(perp_market.decoded.amm.oracle_source, OracleSource::Prelaunch) && writable;
    let oracle_acct_info = oracle.account.to_account_info(
      perp_market.decoded.amm.oracle,
      false,
      oracle_writable,
      false,
    );
    oracle_account_map.insert(perp_market.decoded.amm.oracle.to_string(), oracle_acct_info);

    self.add_spot_market_to_remaining_accounts_map(
      cache,
      perp_market.decoded.quote_spot_market_index,
      false,
      oracle_account_map,
      spot_market_account_map,
    ).await?;

    Ok(())
  }


  /// https://github.com/drift-labs/protocol-v2/blob/6808189602a5f255905018f769ca01bc0344a4bc/sdk/src/driftClient.ts#L1689
  pub async fn remaining_account_maps_for_users(
    &self,
    cache: &RwLockReadGuard<'_, Cache>,
    users: &[User],
  ) -> anyhow::Result<RemainingAccountMaps> {
    let mut oracle_account_map: HashMap<String, AccountInfo> = HashMap::new();
    let mut spot_market_account_map: HashMap<u16, AccountInfo> = HashMap::new();
    let mut perp_market_account_map: HashMap<u16, AccountInfo> = HashMap::new();

    for user in users.iter() {
      for spot_position in user.spot_positions {
        if DriftUtils::spot_position_available(&spot_position) {
          self.add_spot_market_to_remaining_accounts_map(
            cache,
            spot_position.market_index,
            false,
            &mut oracle_account_map,
            &mut spot_market_account_map,
          ).await?;

          if spot_position.open_asks != 0 || spot_position.open_bids != 0 {
            self.add_spot_market_to_remaining_accounts_map(
              cache,
              QUOTE_SPOT_MARKET_INDEX,
              false,
              &mut oracle_account_map,
              &mut spot_market_account_map,
            ).await?;
          }
        }
      }

      for perp_position in user.perp_positions {
        if !DriftUtils::perp_position_available(&perp_position) {
          self.add_perp_market_to_remaining_accounts_map(
            cache,
            perp_position.market_index,
            false,
            &mut oracle_account_map,
            &mut spot_market_account_map,
            &mut perp_market_account_map,
          ).await?;
        }
      }
    }

    Ok(RemainingAccountMaps {
      oracle_account_map,
      spot_market_account_map,
      perp_market_account_map,
    })
  }

  /// https://github.com/drift-labs/protocol-v2/blob/6808189602a5f255905018f769ca01bc0344a4bc/sdk/src/driftClient.ts#L1519
  pub async fn remaining_accounts(
    &self,
    cache: &RwLockReadGuard<'_, Cache>,
    params: RemainingAccountParams,
  ) -> anyhow::Result<Vec<AccountInfo<'static>>> {
    let RemainingAccountMaps {
      mut oracle_account_map,
      mut spot_market_account_map,
      mut perp_market_account_map
    } = self.remaining_account_maps_for_users(cache, params.user_accounts.as_slice()).await?;

    let user_key = DriftUtils::user_pda(&self.signer.pubkey(), 0);
    if params.use_market_last_slot_cache {
      let last_user_slot = cache.account(&user_key, None)?.slot;
      for perp_market in cache.registry_accounts::<PerpMarket>(&CacheKeyRegistry::PerpMarkets, None)? {
        // if cache has more recent slot than user positions account slot, add market to remaining accounts
        // otherwise remove from slot
        if perp_market.slot > last_user_slot {
          self.add_perp_market_to_remaining_accounts_map(
            cache,
            perp_market.decoded.market_index,
            false,
            &mut oracle_account_map,
            &mut spot_market_account_map,
            &mut perp_market_account_map,
          ).await?;
        }
      }

      for spot_market in cache.registry_accounts::<SpotMarket>(&CacheKeyRegistry::SpotMarkets, None)? {
        // if cache has more recent slot than user positions account slot, add market to remaining accounts
        // otherwise remove from slot
        if spot_market.slot > last_user_slot {
          self.add_spot_market_to_remaining_accounts_map(
            cache,
            spot_market.decoded.market_index,
            false,
            &mut oracle_account_map,
            &mut spot_market_account_map,
          ).await?;
        }
      }
    }

    if let Some(readable_perp_market_indexes) = params.readable_perp_market_indexes {
      for market_index in readable_perp_market_indexes {
        self.add_perp_market_to_remaining_accounts_map(
          cache,
          market_index,
          false,
          &mut oracle_account_map,
          &mut spot_market_account_map,
          &mut perp_market_account_map,
        ).await?;
      }
    }
    // skipping mustIncludePerpMarketIndexes that typescript client does

    if let Some(readable_spot_market_indexes) = params.readable_spot_market_indexes {
      for market_index in readable_spot_market_indexes {
        self.add_spot_market_to_remaining_accounts_map(
          cache,
          market_index,
          false,
          &mut oracle_account_map,
          &mut spot_market_account_map,
        ).await?;
      }
    }
    // skipping mustIncludeSpotMarketIndexes that typescript client does

    if let Some(writable_perp_market_indexes) = params.writable_perp_market_indexes {
      for market_index in writable_perp_market_indexes {
        self.add_perp_market_to_remaining_accounts_map(
          cache,
          market_index,
          true,
          &mut oracle_account_map,
          &mut spot_market_account_map,
          &mut perp_market_account_map,
        ).await?;
      }
    }

    if let Some(writable_spot_market_indexes) = params.writable_spot_market_indexes {
      for market_index in writable_spot_market_indexes {
        self.add_spot_market_to_remaining_accounts_map(
          cache,
          market_index,
          true,
          &mut oracle_account_map,
          &mut spot_market_account_map,
        ).await?;
      }
    }

    let mut metas: Vec<AccountInfo<'static>> = vec![];
    metas.extend(oracle_account_map.into_values().collect::<Vec<_>>());
    metas.extend(spot_market_account_map.into_values().collect::<Vec<_>>());
    metas.extend(perp_market_account_map.into_values().collect::<Vec<_>>());

    Ok(metas)
  }

  pub async fn create_token_account(&self, mint: &Pubkey, owner: &Pubkey) -> anyhow::Result<()> {
    let create_ix = spl_associated_token_account::instruction::create_associated_token_account_idempotent(
      owner,
      owner,
      mint,
      &spl_token::id(),
    );
    let sig = TrxBuilder::new(
      self.rpc.clone(),
      false,
      vec![],
    ).with_ixs(vec![create_ix]).send(
      &self.signer,
      &vec![self.signer.deref()],
      spl_token::id(),
    ).await?;
    info!("{:#?}", sig);
    Ok(())
  }

  /// Before starting the websocket subscription, we need to create the User and UserStats accounts.
  pub async fn setup_user(&self) -> anyhow::Result<()> {
    let usdc_ta_key = spl_associated_token_account::get_associated_token_address(
      &self.signer.pubkey(),
      &QUOTE_SPOT_MARKET_MINT,
    );
    let usdc_ta_acct = self.rpc.get_token_account(&usdc_ta_key).await?;
    if usdc_ta_acct.is_none() {
      self.create_token_account(&QUOTE_SPOT_MARKET_MINT, &self.signer.pubkey()).await?;
      info!("Created user USDC token account");
    }
    let usdc_ta_acct = self.rpc.get_token_account(&usdc_ta_key).await?.ok_or(anyhow::anyhow!("USDC token account not created"))?;
    info!("USDC token key: {}", usdc_ta_key);
    let usdc_amount = usdc_ta_acct.token_amount.amount.parse::<u64>()?;
    info!("USDC token amount: {}", usdc_amount as f64 / QUOTE_PRECISION as f64);

    let lamports = self.rpc.get_balance(&self.signer.pubkey()).await?;
    let sol_amount = lamports as f64 / LAMPORTS_PER_SOL as f64;
    info!("SOL token amount: {}", sol_amount);

    let user_acct = self.rpc.get_account_with_commitment(&self.sub_account, CommitmentConfig::confirmed()).await?.value;
    if user_acct.is_none() {
      let mut create_user_trx = TrxBuilder::new(
        self.rpc.clone(),
        false,
        vec![self.program_data.lookup_table.clone()],
      );
      self.initialize_user_stats_ix(&mut create_user_trx).await?;
      self.initialize_user_ix(0, "Arbiter", &mut create_user_trx).await?;
      create_user_trx.send(&self.signer, &vec![self.signer.deref()], id()).await?;
      info!("Created user");
    }

    let user_acct = self.rpc.get_account(&self.sub_account).await?;
    let user = AccountType::decode(user_acct.data.as_slice()).map_err(
      |e| anyhow::anyhow!("Failed to decode User account: {:?}", e)
    )?;
    if let AccountType::User(user) = user {
      if usdc_amount > 0 {
        let mut trx = TrxBuilder::new(
          self.rpc.clone(),
          false,
          vec![self.program_data.lookup_table.clone()],
        );
        self.deposit_ix(&user, usdc_amount, QUOTE_SPOT_MARKET_INDEX, usdc_ta_key, None, &mut trx).await?;
        trx.send(&self.signer, &vec![self.signer.deref()], id()).await?;
      }
    }
    Ok(())
  }

  pub async fn initialize_user_stats_ix(&self, trx: &mut TrxBuilder) -> anyhow::Result<()> {
    let accounts = accounts::InitializeUserStats {
      user_stats: DriftUtils::user_stats_pda(&self.signer.pubkey()),
      state: DriftUtils::state_pda(),
      authority: self.signer.pubkey(),
      payer: self.signer.pubkey(),
      rent: solana_sdk::rent::Rent::id(),
      system_program: solana_sdk::system_program::id(),
    };

    trx.add_ixs(vec![Instruction {
      program_id: id(),
      accounts: accounts.to_account_metas(None),
      data: instruction::InitializeUserStats.data(),
    }]);

    Ok(())
  }

  pub async fn initialize_user_ix(
    &self,
    sub_acct_id: u16,
    name: &str,
    trx: &mut TrxBuilder,
  ) -> anyhow::Result<()> {
    let accounts = accounts::InitializeUser {
      user: DriftUtils::user_pda(&self.signer.pubkey(), sub_acct_id),
      user_stats: DriftUtils::user_stats_pda(&self.signer.pubkey()),
      state: DriftUtils::state_pda(),
      authority: self.signer.pubkey(),
      payer: self.signer.pubkey(),
      rent: solana_sdk::rent::Rent::id(),
      system_program: solana_sdk::system_program::id(),
    };

    let data = instruction::InitializeUser {
      _sub_account_id: sub_acct_id,
      _name: DriftUtils::encode_name(name),
    };

    trx.add_ixs(vec![Instruction {
      program_id: id(),
      accounts: accounts.to_account_metas(None),
      data: data.data(),
    }]);

    Ok(())
  }

  /// Deposit collateral into account
  pub async fn deposit_ix(
    &self,
    user: &User,
    amount: u64,
    spot_market_index: u16,
    user_token_account: Pubkey,
    reduce_only: Option<bool>,
    trx: &mut TrxBuilder,
  ) -> anyhow::Result<()> {
    let accounts = self.build_accounts(
      accounts::Deposit {
        state: DriftUtils::state_pda(),
        user: self.sub_account,
        user_stats: DriftUtils::user_stats_pda(&self.signer.pubkey()),
        authority: self.signer.pubkey(),
        spot_market_vault: DriftUtils::spot_market_vault(spot_market_index),
        user_token_account,
        token_program: TOKEN_PROGRAM_ID,
      },
      &[user],
      &[],
      &[MarketId::spot(spot_market_index)],
    );

    trx.add_ixs(vec![Instruction {
      program_id: id(),
      accounts,
      data: instruction::Deposit {
        _market_index: spot_market_index,
        _amount: amount,
        _reduce_only: reduce_only.unwrap_or(false),
      }.data(),
    }]);

    Ok(())
  }

  /// https://github.com/drift-labs/drift-rs/blob/main/src/lib.rs#L1208
  pub async fn copy_place_orders_ix(
    &self,
    cache: &RwLock<Cache>,
    copy_user: &Pubkey,
    params: Vec<OrderParams>,
    market_filter: Option<&[MarketId]>,
    trx: &mut TrxBuilder,
  ) -> anyhow::Result<()> {
    let cache = cache.read().await;
    let user = cache.decoded_account::<User>(&self.sub_account, None)?.decoded;
    // todo
    let _copy_user = cache.decoded_account::<User>(copy_user, None)?.decoded;

    let state = DriftUtils::state_pda();
    let readable_accounts: Vec<MarketId> = params.iter().map(|o| (o.market_index, o.market_type).into()).collect();

    let accounts = accounts::PlaceOrders {
      state,
      user: self.sub_account,
      authority: self.signer.pubkey(),
    };

    let accounts = self.build_accounts(
      accounts,
      &[&user],
      readable_accounts.as_ref(),
      &[],
    );

    let mut orders_by_market: HashMap<u16, Vec<OrderParams>> = HashMap::new();

    for param in params {
      if let Some(filter) = &market_filter {
        if !filter.contains(&MarketId::from((param.market_index, param.market_type))) {
          continue;
        }
      }
      let orders = orders_by_market.get_mut(&param.market_index);
      if let Some(orders) = orders {
        orders.push(param);
      } else {
        orders_by_market.insert(param.market_index, vec![param]);
      }
    }

    for v in orders_by_market.values_mut() {
      let total = v.iter().map(|o| o.base_asset_amount).sum::<u64>();
      debug!("total: {}, across {} orders", trunc!(total as f64 / BASE_PRECISION as f64, 2), v.len());

      // there might be a bracket of multiple orders for the same market mint
      // to copy a trade accurately we must replicate the ratio of balances in each order
      // but relative to our available assets
      for o in v {
        let ratio = o.base_asset_amount as f64 / total as f64;
        let (sm, price) = match o.market_type {
          MarketType::Perp => {
            let pm = cache.decoded_account::<PerpMarket>(&DriftUtils::perp_market_pda(o.market_index), None)?.decoded;
            let sm = cache.decoded_account::<SpotMarket>(&DriftUtils::spot_market_pda(pm.quote_spot_market_index), None)?;
            let price = self.perp_market_price(&cache, pm.market_index)?;
            (sm, price)
          }
          MarketType::Spot => {
            let sm = cache.decoded_account::<SpotMarket>(&DriftUtils::spot_market_pda(o.market_index), None)?;
            let price = self.spot_market_price(&cache, sm.decoded.market_index)?;
            (sm, price)
          }
        };
        let spot_pos = user.spot_positions.iter().find(|p| p.market_index == sm.decoded.market_index).ok_or(anyhow::anyhow!("User has no position in spot market {}", sm.key))?;
        let quote_balance = spot_pos.cumulative_deposits as f64 / QUOTE_PRECISION as f64;
        let scaled_quote_balance = quote_balance * ratio;
        info!("quote amt: {}", trunc!(scaled_quote_balance, 2));
        let scaled_base_amount = scaled_quote_balance / price.price;
        let base_amount = (scaled_base_amount * BASE_PRECISION as f64).round() as u64;
        o.base_asset_amount = base_amount;
      }
    }

    let _params: Vec<OrderParams> = orders_by_market.values().flatten().cloned().collect();
    let data = instruction::PlaceOrders {
      _params
    };

    trx.add_ixs(vec![Instruction {
      program_id: id(),
      accounts: accounts.to_account_metas(None),
      data: data.data(),
    }]);

    Ok(())
  }

  pub fn perp_market_price(
    &self,
    cache: &RwLockReadGuard<'_, Cache>,
    perp_market_index: u16,
  ) -> anyhow::Result<OraclePrice> {
    // let cache = cache.read().await;
    let pm_key = DriftUtils::perp_market_pda(perp_market_index);
    let pm = cache.decoded_account::<PerpMarket>(&pm_key, None)?.decoded;
    let oracle_key = pm.amm.oracle;
    let oracle_source = pm.amm.oracle_source;
    let oracle_acct = cache.account(&oracle_key, None)?.account.clone();

    let oracle_acct_info = oracle_acct.to_account_info(
      oracle_key,
      false,
      false,
      false,
    );

    let price_data = get_oracle_price(
      &oracle_source,
      &oracle_acct_info,
      cache.slot(),
    ).map_err(|e| anyhow::anyhow!("Failed to get oracle price: {:?}", e))?;
    let price = price_data.price as f64 / PRICE_PRECISION as f64;

    Ok(OraclePrice {
      price,
      name: DriftUtils::decode_name(&pm.name),
    })
  }

  pub fn spot_market_price(
    &self,
    cache: &RwLockReadGuard<'_, Cache>,
    spot_market_index: u16,
  ) -> anyhow::Result<OraclePrice> {
    let sm_key = DriftUtils::spot_market_pda(spot_market_index);
    let sm = cache.decoded_account::<SpotMarket>(&sm_key, None)?.decoded;
    let oracle_key = sm.oracle;
    let oracle_source = sm.oracle_source;
    let oracle_acct = cache.account(&oracle_key, None)?.account.clone();
    let oracle_acct_info = oracle_acct.to_account_info(
      oracle_key,
      false,
      false,
      false,
    );

    let price_data = get_oracle_price(
      &oracle_source,
      &oracle_acct_info,
      cache.slot(),
    ).map_err(|e| anyhow::anyhow!("Failed to get oracle price: {:?}", e))?;
    let price = price_data.price as f64 / PRICE_PRECISION as f64;

    Ok(OraclePrice {
      price,
      name: DriftUtils::decode_name(&sm.name),
    })
  }

  /// Builds a set of required accounts from a user's open positions and additional given accounts
  ///
  /// `base_accounts` base anchor accounts
  ///
  /// `user` Drift user account data
  ///
  /// `markets_readable` IDs of markets to include as readable
  ///
  /// `markets_writable` IDs of markets to include as writable (takes priority over readable)
  ///
  /// # Panics
  ///  if the user has positions in an unknown market (i.e unsupported by the SDK)
  pub fn build_accounts(
    &self,
    base_accounts: impl ToAccountMetas,
    users: &[&User],
    markets_readable: &[MarketId],
    markets_writable: &[MarketId],
  ) -> Vec<AccountMeta> {
    // the order of accounts returned must be instruction, oracles, spot, perps
    // see (https://github.com/drift-labs/protocol-v2/blob/master/programs/drift/src/instructions/optional_accounts.rs#L28)
    let mut seen = [0_u64; 2]; // [spot, perp]
    let mut accounts = Vec::<RemainingAccount>::default();

    // add accounts to the ordered list
    let mut include_market = |market_index: u16, market_type: MarketType, writable: bool| {
      let index_bit = 1_u64 << market_index as u8;
      // always safe since market type is 0 or 1
      let seen_by_type = unsafe { seen.get_unchecked_mut(market_type as usize % 2) };
      if *seen_by_type & index_bit > 0 {
        return;
      }
      *seen_by_type |= index_bit;

      let (account, oracle) = match market_type {
        MarketType::Spot => {
          let SpotMarket { pubkey, oracle, .. } = self.program_data.spot_market_config_by_index(market_index).expect("exists");
          (
            RemainingAccount::Spot {
              pubkey: *pubkey,
              writable,
            },
            oracle,
          )
        }
        MarketType::Perp => {
          let PerpMarket { pubkey, amm, .. } = self.program_data.perp_market_config_by_index(market_index).expect("exists");
          (
            RemainingAccount::Perp {
              pubkey: *pubkey,
              writable,
            },
            &amm.oracle,
          )
        }
      };
      if let Err(idx) = accounts.binary_search(&account) {
        accounts.insert(idx, account);
      }
      let oracle = RemainingAccount::Oracle { pubkey: *oracle };
      if let Err(idx) = accounts.binary_search(&oracle) {
        accounts.insert(idx, oracle);
      }
    };

    for MarketId { index, kind } in markets_writable {
      include_market(*index, *kind, true);
    }

    for MarketId { index, kind } in markets_readable {
      include_market(*index, *kind, false);
    }

    for user in users {
      // Drift program performs margin checks which requires reading user positions
      for p in user.spot_positions.iter().filter(|p| !DriftUtils::spot_position_available(p)) {
        include_market(p.market_index, MarketType::Spot, false);
      }
      for p in user.perp_positions.iter().filter(|p| !DriftUtils::perp_position_available(p)) {
        include_market(p.market_index, MarketType::Perp, false);
      }
    }
    // always manually try to include the quote (USDC) market
    // TODO: this is not exactly the same semantics as the TS sdk
    include_market(MarketId::QUOTE_SPOT.index, MarketType::Spot, false);

    let mut account_metas = base_accounts.to_account_metas(None);
    account_metas.extend(accounts.into_iter().map(Into::into));
    account_metas
  }
}

pub struct DriftUtils;

impl DriftUtils {
  pub fn decode_name(name: &[u8; 32]) -> String {
    String::from_utf8(name.to_vec()).unwrap().trim().to_string()
  }
  pub fn encode_name(name: &str) -> [u8; 32] {
    let mut bytes = [0_u8; 32];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    bytes
  }

  pub fn user_pda(authority: &Pubkey, sub_account_id: u16) -> Pubkey {
    let seeds: &[&[u8]] = &[
      b"user",
      &authority.to_bytes()[..],
      &sub_account_id.to_le_bytes(),
    ];
    Pubkey::find_program_address(seeds, &id()).0
  }

  pub fn user_stats_pda(authority: &Pubkey) -> Pubkey {
    let seeds: &[&[u8]] = &[b"user_stats", &authority.to_bytes()[..]];
    Pubkey::find_program_address(seeds, &id()).0
  }

  pub fn spot_market_pda(market_index: u16) -> Pubkey {
    let seeds: &[&[u8]] = &[b"spot_market", &market_index.to_le_bytes()];
    Pubkey::find_program_address(seeds, &id()).0
  }

  /// calculate the PDA for a drift spot market vault given index
  pub fn spot_market_vault(market_index: u16) -> Pubkey {
    Pubkey::find_program_address(
      &[&b"spot_market_vault"[..], &market_index.to_le_bytes()],
      &id(),
    ).0
  }

  pub fn perp_market_pda(market_index: u16) -> Pubkey {
    let seeds: &[&[u8]] = &[b"perp_market", &market_index.to_le_bytes()];
    Pubkey::find_program_address(seeds, &id()).0
  }

  pub fn state_pda() -> Pubkey {
    let seeds: &[&[u8]] = &[b"drift_state"];
    Pubkey::find_program_address(seeds, &id()).0
  }

  pub fn spot_balance(
    token_amount: u128,
    spot_market: &SpotMarket,
    balance_type: &SpotBalanceType,
    round_up: bool,
  ) -> anyhow::Result<TokenBalance> {
    let precision_increase = 10_u128.pow(
      19_u32.checked_sub(spot_market.decimals).ok_or(anyhow::anyhow!("Checked sub overflow"))?,
    );

    let cumulative_interest = match balance_type {
      SpotBalanceType::Deposit => spot_market.cumulative_deposit_interest,
      SpotBalanceType::Borrow => spot_market.cumulative_borrow_interest,
    };

    let mut balance = token_amount.checked_mul(precision_increase).ok_or(anyhow::anyhow!("Checked mul overflow"))?.checked_div(cumulative_interest).ok_or(anyhow::anyhow!("Checked div overflow"))?;

    if round_up && balance != 0 {
      balance = balance.checked_add(1).ok_or(anyhow::anyhow!("Checked add overflow"))?;
    }

    Ok(TokenBalance {
      balance,
      mint: spot_market.mint,
    })
  }

  pub async fn perp_markets(client: &RpcClient) -> anyhow::Result<Vec<DecodedAcctCtx<PerpMarket>>> {
    let state_key = DriftUtils::state_pda();
    let state_data = client.get_account_data(&state_key).await?;
    let state = State::try_deserialize(&mut state_data.as_slice())?;
    let pdas: Vec<Pubkey> = (0..state.number_of_markets).map(DriftUtils::perp_market_pda).collect();

    let res = client.get_multiple_accounts_with_commitment(&pdas, CommitmentConfig::confirmed()).await?;
    let keyed_accounts = res.value;
    let slot = res.context.slot;
    let valid_accounts: Vec<Account> = keyed_accounts.into_iter().flatten().collect();
    let markets: Vec<DecodedAcctCtx<PerpMarket>> = valid_accounts.into_iter().enumerate().flat_map(|(i, a)| {
      let mut bytes = &a.data.as_slice()[8..];
      match PerpMarket::deserialize(&mut bytes) {
        Ok(market) => Some(DecodedAcctCtx {
          key: pdas[i],
          account: a,
          slot,
          decoded: market,
        }),
        Err(_) => None,
      }
    }).collect();
    Ok(markets)
  }

  pub async fn spot_markets(client: &RpcClient) -> anyhow::Result<Vec<DecodedAcctCtx<SpotMarket>>> {
    let state_key = DriftUtils::state_pda();
    let state_data = client.get_account_data(&state_key).await?;
    let state = State::try_deserialize(&mut state_data.as_slice())?;
    let pdas: Vec<Pubkey> = (0..state.number_of_spot_markets).map(DriftUtils::spot_market_pda).collect();

    let res = client.get_multiple_accounts_with_commitment(&pdas, CommitmentConfig::confirmed()).await?;
    let keyed_accounts = res.value;
    let slot = res.context.slot;
    let valid_accounts: Vec<Account> = keyed_accounts.into_iter().flatten().collect();
    let markets: Vec<DecodedAcctCtx<SpotMarket>> = valid_accounts.into_iter().enumerate().flat_map(|(i, a)| {
      let mut bytes = &a.data.as_slice()[8..];
      match SpotMarket::deserialize(&mut bytes) {
        Ok(market) => Some(DecodedAcctCtx {
          key: pdas[i],
          account: a,
          slot,
          decoded: market,
        }),
        Err(_) => None,
      }
    }).collect();
    Ok(markets)
  }

  pub async fn users(rpc: &RpcClient) -> anyhow::Result<Vec<DecodedAcctCtx<User>>> {
    let discrim = User::discriminator();
    let memcmp = Memcmp::new_base58_encoded(0, discrim.to_vec().as_slice());
    let filters = vec![RpcFilterType::Memcmp(memcmp)];
    let account_config = RpcAccountInfoConfig {
      encoding: Some(UiAccountEncoding::Base64),
      ..Default::default()
    };
    let config = RpcProgramAccountsConfig {
      filters: Some(filters),
      account_config,
      with_context: Some(true),
    };

    let response = rpc.send::<OptionalContext<Vec<RpcKeyedAccount>>>(
      RpcRequest::GetProgramAccounts,
      serde_json::json!([crate::drift_cpi::id().to_string(), config]),
    ).await?;

    let mut users = vec![];
    if let OptionalContext::Context(accounts) = response {
      for account in accounts.value {
        let slot = accounts.context.slot;
        let market_data = account.account.data.clone();
        users.push(DecodedAcctCtx {
          key: Pubkey::from_str(&account.pubkey)?,
          account: account.account.to_account()?,
          slot,
          decoded: decode_ui_account::<User>(market_data)?,
        });
      }
    }
    Ok(users)
  }

  pub async fn user_stats(rpc: &RpcClient, user_auths: &[Pubkey]) -> anyhow::Result<Vec<DecodedAcctCtx<UserStats>>> {
    let pdas = user_auths.iter().map(DriftUtils::user_stats_pda).collect::<Vec<Pubkey>>();

    let account_infos = Nexus::accounts(rpc, &pdas).await?;
    let user_stats: Vec<DecodedAcctCtx<UserStats>> = account_infos.into_par_iter().flat_map(|k| {
      match AccountType::decode(k.account.data.as_slice()).map_err(
        |e| anyhow::anyhow!("Failed to decode account: {:?}", e)
      ) {
        Ok(account) => {
          match account {
            AccountType::UserStats(user) => Some(DecodedAcctCtx {
              key: k.key,
              account: k.account,
              slot: k.slot,
              decoded: user,
            }),
            _ => None,
          }
        }
        Err(e) => {
          log::error!("{:#?}", e);
          None
        }
      }
    }).collect();

    Ok(user_stats)
  }

  /// Gets all Drift users, sorts by highest ROI (perp pnl / deposits), and takes top 1,000 users.
  /// Fetches those 1,000 users' [`UserStats`] accounts to derive "PnL to volume ratio",
  /// and filters out users who have not traded in the last 30 days.
  /// Since one authority can have many User accounts, we map all User accounts to each authority and return.
  pub async fn top_traders(rpc: &RpcClient) -> anyhow::Result<HashMap<Pubkey, DriftTrader>> {
    let start = Instant::now();
    let mut users = DriftUtils::users(rpc).await?;
    let end = Instant::now();
    info!(
        "Fetched Drift {} users in {}s",
        &users.len(),
        trunc!(end.duration_since(start).as_secs_f64(), 2)
    );

    // sort where highest roi is first index
    users.retain(|u| u.decoded.total_deposits > 0);
    users.par_sort_by_key(|a| a.decoded.settled_perp_pnl);

    // map all User accounts to each authority
    let mut user_auths = HashMap::<Pubkey, Vec<DecodedAcctCtx<User>>>::new();
    users.into_iter().for_each(|u| match user_auths.get_mut(&u.decoded.authority) {
      Some(users) => {
        users.push(u);
      }
      None => {
        user_auths.insert(u.decoded.authority, vec![u]);
      }
    });

    // get UserStats account for each authority
    let auths = user_auths.keys().cloned().collect::<Vec<Pubkey>>();
    let user_stats = DriftUtils::user_stats(rpc, auths.as_slice()).await?;

    // UserStat account is PDA of authority pubkey, so there's only ever 1:1.
    // There is never a case when traders HashMap has an existing entry that needs to be updated.
    // Therefore, insert (which overwrites) is safe.
    let mut traders = HashMap::<Pubkey, DriftTrader>::new();
    // filter traders who have traded in the last 30 days
    user_stats.into_iter().filter(|us| us.decoded.taker_volume30d > 0 && us.decoded.maker_volume30d > 0).for_each(|us| {
      let users: Vec<DecodedAcctCtx<User>> = user_auths.remove(&us.decoded.authority).unwrap_or_default();
      let key = us.decoded.authority;
      let trader = DriftTrader {
        authority: us.decoded.authority,
        user_stats: us,
        users,
      };
      traders.insert(key, trader);
    });
    Ok(traders)
  }

  /// Top perp traders, sorted by ROI as a ratio of settled perp pnl to total deposits.
  pub async fn top_traders_by_pnl(rpc: &RpcClient) -> anyhow::Result<Vec<DriftTrader>> {
    let traders_map = DriftUtils::top_traders(rpc).await?;
    let mut traders = traders_map.into_values().collect::<Vec<DriftTrader>>();
    traders.retain(|t| t.settled_perp_pnl() > 0_f64);
    traders.sort_by_key(|a| a.settled_perp_pnl() as i64);
    Ok(traders)
  }

  /// Formatted into [`TraderStats`] struct for easy display and less memory usage.
  pub async fn top_trader_stats_by_pnl(rpc: &RpcClient) -> anyhow::Result<Vec<TraderStats>> {
    let best_traders = DriftUtils::top_traders_by_pnl(rpc).await?;
    let mut trader_stats: Vec<TraderStats> = best_traders.into_iter().map(TraderStats::from).collect();
    trader_stats.sort_by_key(|a| a.settled_perp_pnl as i64);
    Ok(trader_stats)
  }

  pub async fn drift_historical_pnl(nexus: &Nexus, user: &Pubkey, days_back: i64) -> anyhow::Result<HistoricalPerformance> {
    let end = Time::now();
    // drift doesn't have anything more recent than 2 days ago
    let end = end.delta_date(-2);

    let mut data = vec![];
    for i in 0..days_back {
      let date = end.delta_date(-i);

      let url = format!(
        "{}user/{}/settlePnlRecords/{}/{}{}{}",
        DRIFT_API_PREFIX,
        user,
        date.year,
        date.year,
        date.month.to_mm(),
        date.day.to_dd()
      );

      let res = nexus.client.get(url.clone()).header("Accept-Encoding", "gzip").send().await?;
      if res.status().is_success() {
        let bytes = res.bytes().await?;
        let decoder = flate2::read::GzDecoder::new(bytes.as_ref());
        let mut rdr = csv::ReaderBuilder::new().from_reader(decoder);

        for result in rdr.records() {
          let record = result?;
          let datum = record.deserialize::<HistoricalSettlePnl>(None)?;
          data.push(datum);
        }
      } else if res.status() != 403 {
        log::error!(
          "Failed to get historical Drift data with status: {}, for user {} and date: {}/{}/{}",
          res.status(),
          user,
          date.year,
          date.month.to_mm(),
          date.day.to_dd()
        );
      }
    }
    // sort data so latest `ts` field (timestamp) is last index
    data.sort_by_key(|a| a.ts);

    Ok(HistoricalPerformance(data))
  }

  pub async fn perp_market_info(rpc: &RpcClient, perp_market_index: u16) -> anyhow::Result<OraclePrice> {
    let market_pda = DriftUtils::perp_market_pda(perp_market_index);
    let market_acct = rpc.get_account(&market_pda).await?;
    let mut bytes = &market_acct.data.as_slice()[8..];
    let perp_market = PerpMarket::deserialize(&mut bytes).map_err(|_| anyhow::anyhow!("Failed to deserialize perp market"))?;

    let oracle = perp_market.amm.oracle;
    let res = rpc.get_account_with_commitment(&oracle, CommitmentConfig::default()).await?;
    let oracle_acct = res.value.ok_or(anyhow::anyhow!("Oracle account not found"))?;
    let slot = res.context.slot;
    let oracle_source = perp_market.amm.oracle_source;
    let mut data = oracle_acct.data;
    let mut lamports = oracle_acct.lamports;
    let oracle_acct_info = AccountInfo::new(
      &oracle,
      false,
      false,
      &mut lamports,
      &mut data,
      &oracle_acct.owner,
      oracle_acct.executable,
      oracle_acct.rent_epoch,
    );
    let price_data = get_oracle_price(
      &oracle_source,
      &oracle_acct_info,
      slot,
    ).map_err(|e| anyhow::anyhow!("Failed to get oracle price: {:?}", e))?;
    let price = price_data.price as f64 / PRICE_PRECISION as f64;

    Ok(OraclePrice {
      price,
      name: DriftUtils::decode_name(&perp_market.name),
    })
  }

  pub fn spot_position_available(pos: &SpotPosition) -> bool {
    pos.scaled_balance == 0 && pos.open_orders == 0
  }

  pub fn perp_position_available(pos: &PerpPosition) -> bool {
    !DriftUtils::perp_is_open_position(pos) && !DriftUtils::perp_has_open_order(pos) && !DriftUtils::has_unsettled_pnl(pos) && !DriftUtils::perp_is_lp(pos)
  }
  fn perp_is_open_position(pos: &PerpPosition) -> bool {
    pos.base_asset_amount != 0
  }
  fn perp_has_open_order(pos: &PerpPosition) -> bool {
    pos.open_orders != 0 || pos.open_bids != 0 || pos.open_asks != 0
  }
  fn perp_is_lp(pos: &PerpPosition) -> bool {
    pos.lp_shares > 0
  }
  fn has_unsettled_pnl(pos: &PerpPosition) -> bool {
    pos.base_asset_amount == 0 && pos.quote_asset_amount != 0
  }

  /// Fetch all market accounts from drift program (does not require `getProgramAccounts` RPC which is often unavailable)
  pub async fn market_accounts(
    client: &RpcClient,
  ) -> anyhow::Result<(Vec<SpotMarket>, Vec<PerpMarket>)> {
    let state_key = DriftUtils::state_pda();
    let state_data = client.get_account_data(&state_key).await?;
    let state = State::try_deserialize(&mut state_data.as_slice())?;
    let spot_market_pdas: Vec<Pubkey> = (0..state.number_of_spot_markets).map(DriftUtils::spot_market_pda).collect();
    let perp_market_pdas: Vec<Pubkey> = (0..state.number_of_markets).map(DriftUtils::perp_market_pda).collect();

    let (spot_markets, perp_markets) = tokio::join!(
        client.get_multiple_accounts(spot_market_pdas.as_slice()),
        client.get_multiple_accounts(perp_market_pdas.as_slice())
    );

    let spot_markets = spot_markets?.into_iter().map(|x| {
      let account = x.unwrap();
      SpotMarket::try_deserialize(&mut account.data.as_slice()).unwrap()
    }).collect();

    let perp_markets = perp_markets?.into_iter().map(|x| {
      let account = x.unwrap();
      PerpMarket::try_deserialize(&mut account.data.as_slice()).unwrap()
    }).collect();

    Ok((spot_markets, perp_markets))
  }
}