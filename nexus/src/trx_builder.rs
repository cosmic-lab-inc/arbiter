use std::borrow::Cow;
use crate::*;
use crate::drift_cpi::*;
use std::collections::HashMap;
use anchor_lang::prelude::{AccountInfo, AccountMeta};
use solana_sdk::commitment_config::CommitmentConfig;
use anchor_lang::{InstructionData, ToAccountMetas};
use solana_account_decoder::UiAccountEncoding;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcSimulateTransactionAccountsConfig, RpcSimulateTransactionConfig};
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::sysvar::SysvarId;
use solana_sdk::transaction::Transaction;
use solana_transaction_status::UiTransactionEncoding;
use solana_sdk::address_lookup_table::AddressLookupTableAccount;
use solana_sdk::instruction::Instruction;
use solana_sdk::message::{Message, v0, VersionedMessage};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::Signer;
use tokio::sync::RwLock;

pub struct TrxBuilder<'a> {
  rpc: &'a RpcClient,
  /// contextual on-chain program data
  program_data: &'a ProgramData,
  /// sub-account data
  account_data: Cow<'a, User>,
  /// the drift subaccount address
  sub_account: Pubkey,
  /// either account authority or account delegate
  authority: Pubkey,
  /// ordered list of instructions
  ixs: Vec<Instruction>,
  /// use legacy transaction mode
  legacy: bool,
  /// add additional lookup tables (v0 only)
  lookup_tables: Vec<AddressLookupTableAccount>,
}

impl<'a> TrxBuilder<'a> {
  /// Initialize a new [`TrxBuilder`] for default signer
  ///
  /// `program_data` program data from chain
  /// `sub_account` drift sub-account address
  /// `account_data` drift sub-account data
  /// `delegated` set true to build tx for delegated signing
  pub fn new<'b>(
    rpc: &'b RpcClient,
    program_data: &'b ProgramData,
    sub_account_id: u16,
    account_data: Cow<'b, User>,
    delegated: bool,
  ) -> anyhow::Result<Self>
    where
      'b: 'a,
  {
    let authority = if delegated {
      account_data.delegate
    } else {
      account_data.authority
    };
    let sub_account = DriftClient::user_pda(&authority, sub_account_id)?;
    Ok(Self {
      rpc,
      authority,
      program_data,
      account_data,
      sub_account,
      ixs: Default::default(),
      lookup_tables: vec![program_data.lookup_table.clone()],
      legacy: false,
    })
  }

  /// Use legacy tx mode
  pub fn legacy(mut self) -> Self {
    self.legacy = true;
    self
  }

  /// Set the tx lookup tables
  pub fn lookup_tables(mut self, lookup_tables: &[AddressLookupTableAccount]) -> Self {
    self.lookup_tables = lookup_tables.to_vec();
    self.lookup_tables
        .push(self.program_data.lookup_table.clone());
    self
  }

  pub fn add_ix(&mut self, ixs: Vec<Instruction>) {
    self.ixs.extend(ixs);
  }

  /// Build the transaction message ready for signing and sending
  pub fn build(self) -> VersionedMessage {
    if self.legacy {
      let message = Message::new(self.ixs.as_ref(), Some(&self.authority));
      VersionedMessage::Legacy(message)
    } else {
      let message = v0::Message::try_compile(
        &self.authority,
        self.ixs.as_slice(),
        self.lookup_tables.as_slice(),
        Default::default(),
      )
        .expect("ok");
      VersionedMessage::V0(message)
    }
  }

  pub async fn add_spot_market_to_remaining_accounts_map(
    &self,
    cache: &RwLock<AccountCache>,
    market_index: u16,
    writable: bool,
    oracle_account_map: &mut HashMap<String, AccountInfo<'static>>,
    spot_market_account_map: &mut HashMap<u16, AccountInfo<'static>>,
  ) -> anyhow::Result<()> {
    let spot_market_key = DriftClient::spot_market_pda(market_index);
    let spot_market = cache.read().await.find_spot_market(&spot_market_key)?.clone();
    let oracle = cache.read().await.find_spot_oracle(&spot_market.decoded.oracle)?.clone();
    let spot_market_acct = spot_market.account;

    let acct_info = to_account_info(
      spot_market_key,
      false,
      writable,
      false,
      spot_market_acct
    );
    spot_market_account_map.insert(spot_market.decoded.market_index, acct_info);

    if spot_market.decoded.oracle != Pubkey::default() {
      let acct_info = to_account_info(
        spot_market.decoded.oracle,
        false,
        false,
        false,
        oracle.account
      );
      oracle_account_map.insert(spot_market.decoded.oracle.to_string(), acct_info);
    }

    Ok(())
  }

  pub async fn add_perp_market_to_remaining_accounts_map(
    &self,
    cache: &RwLock<AccountCache>,
    market_index: u16,
    writable: bool,
    oracle_account_map: &mut HashMap<String, AccountInfo<'static>>,
    spot_market_account_map: &mut HashMap<u16, AccountInfo<'static>>,
    perp_market_account_map: &mut HashMap<u16, AccountInfo<'static>>
  ) -> anyhow::Result<()> {
    let perp_market_key = DriftClient::perp_market_pda(market_index);
    let perp_market = cache.read().await.find_perp_market(&perp_market_key)?.clone();
    let oracle = cache.read().await.find_perp_oracle(&perp_market.decoded.amm.oracle)?.clone();

    let acct_info = to_account_info(
      perp_market_key,
      false,
      writable,
      false,
      perp_market.account
    );
    perp_market_account_map.insert(market_index, acct_info);

    let oracle_writable = matches!(perp_market.decoded.amm.oracle_source, OracleSource::Prelaunch) && writable;
    let oracle_acct_info = to_account_info(
      perp_market.decoded.amm.oracle,
      false,
      oracle_writable,
      false,
      oracle.account
    );
    oracle_account_map.insert(perp_market.decoded.amm.oracle.to_string(), oracle_acct_info);

    self.add_spot_market_to_remaining_accounts_map(
      cache,
      perp_market.decoded.quote_spot_market_index,
      false,
      oracle_account_map,
      spot_market_account_map
    ).await?;

    Ok(())
  }


  /// https://github.com/drift-labs/protocol-v2/blob/6808189602a5f255905018f769ca01bc0344a4bc/sdk/src/driftClient.ts#L1689
  pub async fn remaining_account_maps_for_users(
    &self,
    cache: &RwLock<AccountCache>,
    users: &[User]
  ) -> anyhow::Result<RemainingAccountMaps> {
    let mut oracle_account_map: HashMap<String, AccountInfo> = HashMap::new();
    let mut spot_market_account_map: HashMap<u16, AccountInfo> = HashMap::new();
    let mut perp_market_account_map: HashMap<u16, AccountInfo> = HashMap::new();

    for user in users.iter() {
      for spot_position in user.spot_positions {
        if DriftClient::spot_position_available(&spot_position) {
          self.add_spot_market_to_remaining_accounts_map(
            cache,
            spot_position.market_index,
            false,
            &mut oracle_account_map,
            &mut spot_market_account_map
          ).await?;

          if spot_position.open_asks != 0 || spot_position.open_bids != 0 {
            self.add_spot_market_to_remaining_accounts_map(
              cache,
              QUOTE_SPOT_MARKET_INDEX,
              false,
              &mut oracle_account_map,
              &mut spot_market_account_map
            ).await?;
          }
        }
      }

      for perp_position in user.perp_positions {
        if !DriftClient::perp_position_available(&perp_position) {
          self.add_perp_market_to_remaining_accounts_map(
            cache,
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
    cache: &RwLock<AccountCache>,
    params: RemainingAccountParams
  ) -> anyhow::Result<Vec<AccountInfo<'static>>> {
    let RemainingAccountMaps {
      mut oracle_account_map,
      mut spot_market_account_map,
      mut perp_market_account_map
    } = self.remaining_account_maps_for_users(cache, params.user_accounts.as_slice()).await?;

    let user_key = DriftClient::user_pda(&self.authority, 0)?;
    if params.use_market_last_slot_cache {
      let read_guard = cache.read().await;
      let last_user_slot = read_guard.find_user(&user_key)?.slot;
      for perp_market in read_guard.perp_markets() {
        // if cache has more recent slot than user positions account slot, add market to remaining accounts
        // otherwise remove from slot
        if perp_market.slot > last_user_slot {
          self.add_perp_market_to_remaining_accounts_map(
            cache,
            perp_market.decoded.market_index,
            false,
            &mut oracle_account_map,
            &mut spot_market_account_map,
            &mut perp_market_account_map
          ).await?;
        }
      }

      for spot_market in read_guard.spot_markets() {
        // if cache has more recent slot than user positions account slot, add market to remaining accounts
        // otherwise remove from slot
        if spot_market.slot > last_user_slot {
          self.add_spot_market_to_remaining_accounts_map(
            cache,
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
          cache,
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
          cache,
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
          cache,
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
          cache,
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

  pub async fn initialize_user_stats(&self) -> anyhow::Result<()> {
    let accounts = accounts::InitializeUserStats {
      user_stats: DriftClient::user_stats_pda(&self.authority)?,
      state: DriftClient::state_pda(),
      authority: self.authority,
      payer: self.authority,
      rent: solana_sdk::rent::Rent::id(),
      system_program: solana_sdk::system_program::id(),
    };

    let data = instruction::InitializeUserStats;

    let mut ixs = vec![];
    self.with_priority_fee(&mut ixs, self.get_recent_priority_fee(id(), None).await?, None)?;
    ixs.push(Instruction {
      program_id: id(),
      accounts: accounts.to_account_metas(None),
      data: data.data()
    });

    Ok(())
  }

  pub async fn initialize_user(&mut self, sub_acct_id: u16, name: &str) -> anyhow::Result<()> {
    let accounts = accounts::InitializeUser {
      user: DriftClient::user_pda(&self.authority, sub_acct_id)?,
      user_stats: DriftClient::user_stats_pda(&self.authority)?,
      state: DriftClient::state_pda(),
      authority: self.authority,
      payer: self.authority,
      rent: solana_sdk::rent::Rent::id(),
      system_program: solana_sdk::system_program::id(),
    };

    let data = instruction::InitializeUser {
      _sub_account_id: sub_acct_id,
      _name: name.as_bytes().try_into()?
    };

    let mut ixs = vec![];
    self.with_priority_fee(&mut ixs, self.get_recent_priority_fee(id(), None).await?, None)?;

    ixs.push(Instruction {
      program_id: id(),
      accounts: accounts.to_account_metas(None),
      data: data.data()
    });
    self.ixs.extend(ixs);

    Ok(())
  }

  /// https://github.com/drift-labs/drift-rs/blob/main/src/lib.rs#L1208
  pub async fn place_orders(&mut self, params: Vec<OrderParams>) -> anyhow::Result<()> {
    let state = DriftClient::state_pda();
    let readable_accounts: Vec<MarketId> = params
      .iter()
      .map(|o| (o.market_index, o.market_type).into())
      .collect();

    let accounts = accounts::PlaceOrders {
      state,
      user: self.sub_account,
      authority: self.authority,
    };

    let accounts = Self::build_accounts(
      self.program_data,
      accounts,
      &[self.account_data.as_ref()],
      readable_accounts.as_ref(),
      &[],
    );

    // todo: calc available balances for markets
    let base_asset_amount = 0;
    let mut order_params: Vec<OrderParams> = vec![];
    for mut param in params {
      param.base_asset_amount = base_asset_amount;
      order_params.push(param);
    }
    let data = instruction::PlaceOrders {
      _params: order_params
    };

    let program_id = id();

    let mut ixs = vec![];

    let prior_fee = self.get_recent_priority_fee(program_id, None).await?;
    self.with_priority_fee(&mut ixs, prior_fee, None)?;

    ixs.push(Instruction {
      program_id,
      accounts: accounts.to_account_metas(None),
      data: data.data()
    });
    self.ixs.extend(ixs);

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
      .rpc
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
    program_data: &ProgramData,
    base_accounts: impl ToAccountMetas,
    users: &[&User],
    markets_readable: &[MarketId],
    markets_writable: &[MarketId],
  ) -> Vec<AccountMeta> {
    // the order of accounts returned must be instruction, oracles, spot, perps see (https://github.com/drift-labs/protocol-v2/blob/master/programs/drift/src/instructions/optional_accounts.rs#L28)
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
          let SpotMarket { pubkey, oracle, .. } = program_data
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
          let PerpMarket { pubkey, amm, .. } = program_data
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
      for p in user.spot_positions.iter().filter(|p| !DriftClient::spot_position_available(p)) {
        include_market(p.market_index, MarketType::Spot, false);
      }
      for p in user.perp_positions.iter().filter(|p| !DriftClient::perp_position_available(p)) {
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

  pub async fn simulate<T: Signer + Sized>(&self, payer: &T) -> anyhow::Result<()> {
    let tx = Transaction::new_signed_with_payer(
      &self.ixs,
      Some(&self.authority),
      &[payer],
      self.rpc.get_latest_blockhash().await?
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
    let res = self.rpc.simulate_transaction_with_config(&tx, config).await?;
    log::info!("simulation: {:#?}", res.value);
    Ok(())
  }

  pub async fn send<T: Signer + Sized>(&self, payer: &T) -> anyhow::Result<()> {
    let tx = Transaction::new_signed_with_payer(
      &self.ixs,
      Some(&self.authority),
      &[payer],
      self.rpc.get_latest_blockhash().await?
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
    let res = self.rpc.simulate_transaction_with_config(&tx, config).await?;
    log::info!("simulation: {:#?}", res.value);
    Ok(())
  }
}