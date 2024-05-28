use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use log::{debug, info};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::account_info::AccountInfo;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::sysvar::SysvarId;
use tokio::sync::{RwLock, RwLockReadGuard};

use crate::*;

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
  pub async fn new(
    signer: Arc<Keypair>,
    rpc: Arc<RpcClient>,
    sub_account_id: u16,
  ) -> anyhow::Result<Self> {
    let (spot_markets, perp_markets) = DriftUtils::market_accounts(&rpc).await?;
    let lut = rpc.get_account(&MARKET_LOOKUP_TABLE).await?;
    let lookup_table = deserialize_lookup_table(MARKET_LOOKUP_TABLE, &lut)?;
    let program_data = ProgramData::new(spot_markets, perp_markets, lookup_table);
    let sub_account = DriftUtils::user_pda(&signer.pubkey(), sub_account_id);
    Ok(Self {
      signer,
      rpc,
      program_data,
      sub_account,
    })
  }

  pub fn new_tx(&self, with_lookup_tables: bool) -> TrxBuilder<'_, Keypair, Vec<&Keypair>> {
    let alt = if with_lookup_tables {
      vec![self.program_data.lookup_table.clone()]
    } else {
      vec![]
    };
    TrxBuilder::<'_, Keypair, Vec<&Keypair>>::new(
      self.rpc.clone(),
      false,
      alt,
      &self.signer,
      vec![self.signer.deref()],
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
    let spot_market = cache
      .decoded_account::<SpotMarket>(&spot_market_key, None)
      .await?;
    let oracle = cache
      .account(&spot_market.decoded.oracle, None)
      .await?
      .clone();
    let spot_market_acct = spot_market.account;

    let acct_info = spot_market_acct.to_account_info(spot_market_key, false, writable, false);
    spot_market_account_map.insert(spot_market.decoded.market_index, acct_info);

    if spot_market.decoded.oracle != Pubkey::default() {
      let acct_info =
        oracle
          .account
          .to_account_info(spot_market.decoded.oracle, false, false, false);
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
    let perp_market = cache
      .decoded_account::<PerpMarket>(&perp_market_key, None)
      .await?;
    let oracle = cache
      .account(&perp_market.decoded.amm.oracle, None)
      .await?
      .clone();

    let acct_info = perp_market
      .account
      .to_account_info(perp_market_key, false, writable, false);
    perp_market_account_map.insert(market_index, acct_info);

    let oracle_writable = matches!(
      perp_market.decoded.amm.oracle_source,
      OracleSource::Prelaunch
    ) && writable;
    let oracle_acct_info = oracle.account.to_account_info(
      perp_market.decoded.amm.oracle,
      false,
      oracle_writable,
      false,
    );
    oracle_account_map.insert(perp_market.decoded.amm.oracle.to_string(), oracle_acct_info);

    self
      .add_spot_market_to_remaining_accounts_map(
        cache,
        perp_market.decoded.quote_spot_market_index,
        false,
        oracle_account_map,
        spot_market_account_map,
      )
      .await?;

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
          self
            .add_spot_market_to_remaining_accounts_map(
              cache,
              spot_position.market_index,
              false,
              &mut oracle_account_map,
              &mut spot_market_account_map,
            )
            .await?;

          if spot_position.open_asks != 0 || spot_position.open_bids != 0 {
            self
              .add_spot_market_to_remaining_accounts_map(
                cache,
                QUOTE_SPOT_MARKET_INDEX,
                false,
                &mut oracle_account_map,
                &mut spot_market_account_map,
              )
              .await?;
          }
        }
      }

      for perp_position in user.perp_positions {
        if !DriftUtils::perp_position_available(&perp_position) {
          self
            .add_perp_market_to_remaining_accounts_map(
              cache,
              perp_position.market_index,
              false,
              &mut oracle_account_map,
              &mut spot_market_account_map,
              &mut perp_market_account_map,
            )
            .await?;
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
      mut perp_market_account_map,
    } = self
      .remaining_account_maps_for_users(cache, params.user_accounts.as_slice())
      .await?;

    let user_key = DriftUtils::user_pda(&self.signer.pubkey(), 0);
    if params.use_market_last_slot_cache {
      let last_user_slot = cache.account(&user_key, None).await?.slot;
      for perp_market in
        cache.registry_accounts::<PerpMarket>(&CacheKeyRegistry::PerpMarkets, None)?
      {
        // if cache has more recent slot than user positions account slot, add market to remaining accounts
        // otherwise remove from slot
        if perp_market.slot > last_user_slot {
          self
            .add_perp_market_to_remaining_accounts_map(
              cache,
              perp_market.decoded.market_index,
              false,
              &mut oracle_account_map,
              &mut spot_market_account_map,
              &mut perp_market_account_map,
            )
            .await?;
        }
      }

      for spot_market in
        cache.registry_accounts::<SpotMarket>(&CacheKeyRegistry::SpotMarkets, None)?
      {
        // if cache has more recent slot than user positions account slot, add market to remaining accounts
        // otherwise remove from slot
        if spot_market.slot > last_user_slot {
          self
            .add_spot_market_to_remaining_accounts_map(
              cache,
              spot_market.decoded.market_index,
              false,
              &mut oracle_account_map,
              &mut spot_market_account_map,
            )
            .await?;
        }
      }
    }

    if let Some(readable_perp_market_indexes) = params.readable_perp_market_indexes {
      for market_index in readable_perp_market_indexes {
        self
          .add_perp_market_to_remaining_accounts_map(
            cache,
            market_index,
            false,
            &mut oracle_account_map,
            &mut spot_market_account_map,
            &mut perp_market_account_map,
          )
          .await?;
      }
    }
    // skipping mustIncludePerpMarketIndexes that typescript client does

    if let Some(readable_spot_market_indexes) = params.readable_spot_market_indexes {
      for market_index in readable_spot_market_indexes {
        self
          .add_spot_market_to_remaining_accounts_map(
            cache,
            market_index,
            false,
            &mut oracle_account_map,
            &mut spot_market_account_map,
          )
          .await?;
      }
    }
    // skipping mustIncludeSpotMarketIndexes that typescript client does

    if let Some(writable_perp_market_indexes) = params.writable_perp_market_indexes {
      for market_index in writable_perp_market_indexes {
        self
          .add_perp_market_to_remaining_accounts_map(
            cache,
            market_index,
            true,
            &mut oracle_account_map,
            &mut spot_market_account_map,
            &mut perp_market_account_map,
          )
          .await?;
      }
    }

    if let Some(writable_spot_market_indexes) = params.writable_spot_market_indexes {
      for market_index in writable_spot_market_indexes {
        self
          .add_spot_market_to_remaining_accounts_map(
            cache,
            market_index,
            true,
            &mut oracle_account_map,
            &mut spot_market_account_map,
          )
          .await?;
      }
    }

    let mut metas: Vec<AccountInfo<'static>> = vec![];
    metas.extend(oracle_account_map.into_values().collect::<Vec<_>>());
    metas.extend(spot_market_account_map.into_values().collect::<Vec<_>>());
    metas.extend(perp_market_account_map.into_values().collect::<Vec<_>>());

    Ok(metas)
  }

  pub async fn create_token_account(&self, mint: &Pubkey, owner: &Pubkey) -> anyhow::Result<()> {
    let create_ix =
      spl_associated_token_account::instruction::create_associated_token_account_idempotent(
        owner,
        owner,
        mint,
        &spl_token::id(),
      );
    let res = self
      .new_tx(false)
      .with_ixs(vec![create_ix])
      .send(spl_token::id(), None)
      .await?;
    if let Err(e) = &res.1 {
      log::error!("Failed to confirm transaction: {:#?}", e);
    }
    info!("{:#?}", res.0);
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
      self
        .create_token_account(&QUOTE_SPOT_MARKET_MINT, &self.signer.pubkey())
        .await?;
      info!("Created user USDC token account");
    }
    let usdc_ta_acct = self
      .rpc
      .get_token_account(&usdc_ta_key)
      .await?
      .ok_or(anyhow::anyhow!("USDC token account not created"))?;
    info!("USDC token key: {}", usdc_ta_key);
    let usdc_amount = usdc_ta_acct.token_amount.amount.parse::<u64>()?;
    info!(
      "USDC token amount: {}",
      usdc_amount as f64 / QUOTE_PRECISION as f64
    );

    let user_acct = self
      .rpc
      .get_account_with_commitment(&self.sub_account, CommitmentConfig::confirmed())
      .await?
      .value;
    if user_acct.is_none() {
      let mut create_user_trx = self.new_tx(false);
      self.initialize_user_stats_ix(&mut create_user_trx).await?;
      self
        .initialize_user_ix(0, "Arbiter", &mut create_user_trx)
        .await?;
      let res = create_user_trx.send(id(), None).await?;
      if let Err(e) = &res.1 {
        log::error!("Failed to confirm transaction: {:#?}", e);
      }
      info!("Created user");
    }

    let user_acct = self.rpc.get_account(&self.sub_account).await?;
    let user = AccountType::decode(user_acct.data.as_slice())
      .map_err(|e| anyhow::anyhow!("Failed to decode User account: {:?}", e))?;
    if let AccountType::User(user) = user {
      if usdc_amount > 0 {
        let mut trx = self.new_tx(false);
        self
          .deposit_ix(
            &user,
            usdc_amount,
            QUOTE_SPOT_MARKET_INDEX,
            usdc_ta_key,
            None,
            &mut trx,
          )
          .await?;
        let res = trx.send(id(), None).await?;
        if let Err(e) = &res.1 {
          log::error!("Failed to confirm transaction: {:#?}", e);
        }
      }
    }
    Ok(())
  }

  // ======================================================================
  // Instructions
  // ======================================================================

  pub async fn initialize_user_stats_ix(
    &self,
    trx: &mut TrxBuilder<'_, Keypair, Vec<&Keypair>>,
  ) -> anyhow::Result<()> {
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
    trx: &mut TrxBuilder<'_, Keypair, Vec<&Keypair>>,
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
    trx: &mut TrxBuilder<'_, Keypair, Vec<&Keypair>>,
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
      }
      .data(),
    }]);

    Ok(())
  }

  /// https://github.com/drift-labs/drift-rs/blob/main/src/lib.rs#L1208
  pub async fn copy_place_orders_ix(
    &self,
    tx_slot: u64,
    cache: &RwLock<Cache>,
    params: Vec<OrderParams>,
    market_filter: Option<&[MarketId]>,
    trx: &mut TrxBuilder<'_, Keypair, Vec<&Keypair>>,
  ) -> anyhow::Result<()> {
    let cache = cache.read().await;
    let user = cache
      .decoded_account::<User>(&self.sub_account, None)
      .await?
      .decoded;

    let state = DriftUtils::state_pda();
    let readable_accounts: Vec<MarketId> = params
      .iter()
      .map(|o| (o.market_index, o.market_type).into())
      .collect();

    let accounts = accounts::PlaceOrders {
      state,
      user: self.sub_account,
      authority: self.signer.pubkey(),
    };

    let accounts = self.build_accounts(accounts, &[&user], readable_accounts.as_ref(), &[]);

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
      debug!(
        "total: {}, across {} orders",
        trunc!(total as f64 / BASE_PRECISION as f64, 2),
        v.len()
      );

      // there might be a bracket of multiple orders for the same market mint
      // to copy a trade accurately we must replicate the ratio of balances in each order
      // but relative to our available assets
      for o in v {
        // todo: remove after debug
        let market_id = MarketId::from((o.market_index, o.market_type));
        let copy_tx_oracle_price = self
          .oracle_price(market_id, &cache, Some(tx_slot), o.oracle_price_offset)
          .await?;
        DriftUtils::log_order(o, &copy_tx_oracle_price, Some("Copy trade"));

        let ratio = o.base_asset_amount as f64 / total as f64;
        let (sm, price) = match o.market_type {
          MarketType::Perp => {
            let pm = cache
              .decoded_account::<PerpMarket>(&DriftUtils::perp_market_pda(o.market_index), None)
              .await?
              .decoded;
            let sm = cache
              .decoded_account::<SpotMarket>(
                &DriftUtils::spot_market_pda(pm.quote_spot_market_index),
                None,
              )
              .await?;
            let price = self.perp_market_price(&cache, pm.market_index).await?;
            (sm, price)
          }
          MarketType::Spot => {
            let sm = cache
              .decoded_account::<SpotMarket>(&DriftUtils::spot_market_pda(o.market_index), None)
              .await?;
            let price = self
              .spot_market_price(&cache, sm.decoded.market_index)
              .await?;
            (sm, price)
          }
        };

        let spot_pos = user
          .spot_positions
          .iter()
          .find(|p| p.market_index == sm.decoded.market_index)
          .ok_or(anyhow::anyhow!(
            "User has no position in spot market {}",
            sm.key
          ))?;
        let quote_balance = spot_pos.cumulative_deposits as f64 / QUOTE_PRECISION as f64;
        let scaled_quote_balance = quote_balance * ratio;
        let scaled_base_amount = scaled_quote_balance / price.price;
        let original_base_amount = trunc!(o.base_asset_amount as f64 / BASE_PRECISION as f64, 2);
        debug!(
          "copy base: {}, ratio: {}, new base: {}",
          original_base_amount,
          trunc!(ratio, 2),
          trunc!(scaled_base_amount, 2)
        );
        let base_amount = (scaled_base_amount * BASE_PRECISION as f64).round() as u64;
        o.base_asset_amount = base_amount;
        o.max_ts = None;

        // if `oracle_price_offset` is set then `price` must == 0.
        // if `oracle_price_offset` exists then set our offset according to the updated oracle price since the copied trx occurred.
        // if no `oracle_price_offset` then do nothing and reuse the `price` from the copied trx.
        if o.oracle_price_offset.is_some() {
          let price_diff_since_copy_tx = copy_tx_oracle_price.price - price.price;
          let new_oracle_offset =
            (price_diff_since_copy_tx * PRICE_PRECISION as f64).round() as i32;
          o.oracle_price_offset = Some(new_oracle_offset);
        }

        let current_oracle_price = self
          .oracle_price(market_id, &cache, None, o.oracle_price_offset)
          .await?;
        DriftUtils::log_order(o, &current_oracle_price, Some("Our trade"));
      }
    }

    let _params: Vec<OrderParams> = orders_by_market.values().flatten().cloned().collect();
    let data = instruction::PlaceOrders { _params };

    trx.add_ixs(vec![Instruction {
      program_id: id(),
      accounts: accounts.to_account_metas(None),
      data: data.data(),
    }]);

    Ok(())
  }

  pub async fn cancel_orders_ix(
    &self,
    cache: &RwLock<Cache>,
    market_filter: Option<&[MarketId]>,
    market: Option<MarketId>,
    direction: Option<PositionDirection>,
    trx: &mut TrxBuilder<'_, Keypair, Vec<&Keypair>>,
  ) -> anyhow::Result<()> {
    if let (Some(filter), Some(market)) = (market_filter, market) {
      if !filter.contains(&market) {
        return Ok(());
      }
    }

    let cache = cache.read().await;
    let user = cache
      .decoded_account::<User>(&self.sub_account, None)
      .await?
      .decoded;

    let accounts = accounts::CancelOrder {
      state: DriftUtils::state_pda(),
      user: self.sub_account,
      authority: self.signer.pubkey(),
    };
    let markets_readable = match market {
      Some(market) => vec![market],
      None => vec![],
    };
    let accounts = self.build_accounts(accounts, &[&user], markets_readable.as_slice(), &[]);

    let ix = match market {
      Some(market) => instruction::CancelOrders {
        _market_index: Some(market.index),
        _market_type: Some(market.kind),
        _direction: direction,
      },
      None => instruction::CancelOrders {
        _market_index: None,
        _market_type: None,
        _direction: None,
      },
    };

    trx.add_ixs(vec![Instruction {
      program_id: id(),
      accounts: accounts.clone(),
      data: ix.data(),
    }]);

    Ok(())
  }

  // ======================================================================
  // Utilities
  // ======================================================================

  pub async fn perp_market_price(
    &self,
    cache: &RwLockReadGuard<'_, Cache>,
    perp_market_index: u16,
  ) -> anyhow::Result<OraclePrice> {
    // let cache = cache.read().await;
    let pm_key = DriftUtils::perp_market_pda(perp_market_index);
    let pm = cache
      .decoded_account::<PerpMarket>(&pm_key, None)
      .await?
      .decoded;
    let oracle_key = pm.amm.oracle;
    let oracle_source = pm.amm.oracle_source;
    let oracle_acct = cache.account(&oracle_key, None).await?.account.clone();

    let oracle_acct_info = oracle_acct.to_account_info(oracle_key, false, false, false);

    let price_data = get_oracle_price(&oracle_source, &oracle_acct_info, cache.block(None)?.slot)
      .map_err(|e| anyhow::anyhow!("Failed to get oracle price: {:?}", e))?;
    let price = price_data.price as f64 / PRICE_PRECISION as f64;

    Ok(OraclePrice {
      price,
      name: DriftUtils::decode_name(&pm.name),
    })
  }

  pub async fn spot_market_price(
    &self,
    cache: &RwLockReadGuard<'_, Cache>,
    spot_market_index: u16,
  ) -> anyhow::Result<OraclePrice> {
    let sm_key = DriftUtils::spot_market_pda(spot_market_index);
    let sm = cache
      .decoded_account::<SpotMarket>(&sm_key, None)
      .await?
      .decoded;
    let oracle_key = sm.oracle;
    let oracle_source = sm.oracle_source;
    let oracle_acct = cache.account(&oracle_key, None).await?.account.clone();
    let oracle_acct_info = oracle_acct.to_account_info(oracle_key, false, false, false);

    let price_data = get_oracle_price(&oracle_source, &oracle_acct_info, cache.block(None)?.slot)
      .map_err(|e| anyhow::anyhow!("Failed to get oracle price: {:?}", e))?;
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
          let SpotMarket { pubkey, oracle, .. } = self
            .program_data
            .spot_market_config_by_index(market_index)
            .expect("exists");
          (
            RemainingAccount::Spot {
              pubkey: *pubkey,
              writable,
            },
            oracle,
          )
        }
        MarketType::Perp => {
          let PerpMarket { pubkey, amm, .. } = self
            .program_data
            .perp_market_config_by_index(market_index)
            .expect("exists");
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
      for p in user
        .spot_positions
        .iter()
        .filter(|p| !DriftUtils::spot_position_available(p))
      {
        include_market(p.market_index, MarketType::Spot, false);
      }
      for p in user
        .perp_positions
        .iter()
        .filter(|p| !DriftUtils::perp_position_available(p))
      {
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

  pub async fn oracle_price(
    &self,
    market: MarketId,
    cache: &RwLockReadGuard<'_, Cache>,
    slot: Option<u64>,
    oracle_offset: Option<i32>,
  ) -> anyhow::Result<OraclePrice> {
    let offset = match oracle_offset {
      None => 0.0,
      Some(offset) => offset as f64 / PRICE_PRECISION as f64,
    };
    let (price, name) = match market.kind {
      MarketType::Spot => {
        let market_key = DriftUtils::spot_market_pda(market.index);
        let market_ctx = cache
          .decoded_account::<SpotMarket>(&market_key, slot)
          .await?;
        let oracle_ctx = cache.account(&market_ctx.decoded.oracle, slot).await?;
        let price = DriftUtils::oracle_price(
          &market_ctx.decoded.oracle_source,
          market_ctx.decoded.oracle,
          &oracle_ctx.account,
          oracle_ctx.slot,
        )?;
        let name = DriftUtils::decode_name(&market_ctx.decoded.name);
        (price, name)
      }
      MarketType::Perp => {
        let market_key = DriftUtils::perp_market_pda(market.index);
        let market_ctx = cache
          .decoded_account::<PerpMarket>(&market_key, slot)
          .await?;
        let oracle_ctx = cache.account(&market_ctx.decoded.amm.oracle, slot).await?;
        let price = DriftUtils::oracle_price(
          &market_ctx.decoded.amm.oracle_source,
          market_ctx.decoded.amm.oracle,
          &oracle_ctx.account,
          oracle_ctx.slot,
        )?;
        let name = DriftUtils::decode_name(&market_ctx.decoded.name);
        (price, name)
      }
    };
    Ok(OraclePrice {
      price: price + offset,
      name,
    })
  }
}
