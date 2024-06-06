use std::collections::HashMap;
use std::str::FromStr;
use std::time::Instant;

use anchor_lang::{AccountDeserialize, Discriminator};
use borsh::BorshDeserialize;
use log::info;
use rayon::prelude::*;
use reqwest::Client;
use solana_account_decoder::UiAccountEncoding;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use solana_client::rpc_filter::{Memcmp, RpcFilterType};
use solana_rpc_client_api::filter::MemcmpEncodedBytes;
use solana_rpc_client_api::request::RpcRequest;
use solana_rpc_client_api::response::{OptionalContext, RpcKeyedAccount};
use solana_sdk::account::Account;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::subscribe_request_filter_accounts_filter::Filter;
use yellowstone_grpc_proto::prelude::{
  subscribe_request_filter_accounts_filter_memcmp, SubscribeRequestFilterAccountsFilter,
  SubscribeRequestFilterAccountsFilterMemcmp,
};

use crate::drift_client::*;
use crate::*;
use crate::{trunc, DecodedAcctCtx, Time};

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
    )
    .0
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
      19_u32
        .checked_sub(spot_market.decimals)
        .ok_or(anyhow::anyhow!("Checked sub overflow"))?,
    );

    let cumulative_interest = match balance_type {
      SpotBalanceType::Deposit => spot_market.cumulative_deposit_interest,
      SpotBalanceType::Borrow => spot_market.cumulative_borrow_interest,
    };

    let mut balance = token_amount
      .checked_mul(precision_increase)
      .ok_or(anyhow::anyhow!("Checked mul overflow"))?
      .checked_div(cumulative_interest)
      .ok_or(anyhow::anyhow!("Checked div overflow"))?;

    if round_up && balance != 0 {
      balance = balance
        .checked_add(1)
        .ok_or(anyhow::anyhow!("Checked add overflow"))?;
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
    let pdas: Vec<Pubkey> = (0..state.number_of_markets)
      .map(DriftUtils::perp_market_pda)
      .collect();

    let res = client
      .get_multiple_accounts_with_commitment(&pdas, CommitmentConfig::confirmed())
      .await?;
    let keyed_accounts = res.value;
    let slot = res.context.slot;
    let valid_accounts: Vec<Account> = keyed_accounts.into_iter().flatten().collect();
    let markets: Vec<DecodedAcctCtx<PerpMarket>> = valid_accounts
      .into_iter()
      .enumerate()
      .flat_map(|(i, a)| {
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
      })
      .collect();
    Ok(markets)
  }

  pub async fn spot_markets(client: &RpcClient) -> anyhow::Result<Vec<DecodedAcctCtx<SpotMarket>>> {
    let state_key = DriftUtils::state_pda();
    let state_data = client.get_account_data(&state_key).await?;
    let state = State::try_deserialize(&mut state_data.as_slice())?;
    let pdas: Vec<Pubkey> = (0..state.number_of_spot_markets)
      .map(DriftUtils::spot_market_pda)
      .collect();

    let res = client
      .get_multiple_accounts_with_commitment(&pdas, CommitmentConfig::confirmed())
      .await?;
    let keyed_accounts = res.value;
    let slot = res.context.slot;
    let valid_accounts: Vec<Account> = keyed_accounts.into_iter().flatten().collect();
    let markets: Vec<DecodedAcctCtx<SpotMarket>> = valid_accounts
      .into_iter()
      .enumerate()
      .flat_map(|(i, a)| {
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
      })
      .collect();
    Ok(markets)
  }

  pub fn users_filter() -> RpcFilterType {
    RpcFilterType::Memcmp(Memcmp::new_base58_encoded(
      0,
      User::discriminator().to_vec().as_slice(),
    ))
  }

  pub fn grpc_users_filter() -> SubscribeRequestFilterAccountsFilter {
    SubscribeRequestFilterAccountsFilter {
      filter: Some(Filter::Memcmp(SubscribeRequestFilterAccountsFilterMemcmp {
        offset: 0,
        data: Some(
          subscribe_request_filter_accounts_filter_memcmp::Data::Base58(
            solana_sdk::bs58::encode(User::discriminator()).into_string(),
          ),
        ),
      })),
    }
  }

  pub fn grpc_perp_markets_filter() -> SubscribeRequestFilterAccountsFilter {
    SubscribeRequestFilterAccountsFilter {
      filter: Some(Filter::Memcmp(SubscribeRequestFilterAccountsFilterMemcmp {
        offset: 0,
        data: Some(
          subscribe_request_filter_accounts_filter_memcmp::Data::Base58(
            solana_sdk::bs58::encode(PerpMarket::discriminator()).into_string(),
          ),
        ),
      })),
    }
  }

  pub fn grpc_spot_markets_filter() -> SubscribeRequestFilterAccountsFilter {
    SubscribeRequestFilterAccountsFilter {
      filter: Some(Filter::Memcmp(SubscribeRequestFilterAccountsFilterMemcmp {
        offset: 0,
        data: Some(
          subscribe_request_filter_accounts_filter_memcmp::Data::Base58(
            solana_sdk::bs58::encode(SpotMarket::discriminator()).into_string(),
          ),
        ),
      })),
    }
  }

  pub fn users_with_order_filter() -> RpcFilterType {
    let filter: String = solana_sdk::bs58::encode(vec![1]).into_string();
    RpcFilterType::Memcmp(Memcmp::new(4352, MemcmpEncodedBytes::Base58(filter)))
  }

  pub fn grpc_subscribe_users_with_order_filter() -> SubscribeRequestFilterAccountsFilter {
    SubscribeRequestFilterAccountsFilter {
      filter: Some(Filter::Memcmp(SubscribeRequestFilterAccountsFilterMemcmp {
        offset: 4352,
        data: Some(
          subscribe_request_filter_accounts_filter_memcmp::Data::Base58(
            solana_sdk::bs58::encode(vec![1]).into_string(),
          ),
        ),
      })),
    }
  }

  pub async fn users(rpc: &RpcClient) -> anyhow::Result<Vec<DecodedAcctCtx<User>>> {
    let filters = Some(vec![Self::users_filter()]);
    let account_config = RpcAccountInfoConfig {
      encoding: Some(UiAccountEncoding::Base64),
      ..Default::default()
    };
    let config = RpcProgramAccountsConfig {
      filters,
      account_config,
      with_context: Some(true),
    };

    let response = rpc
      .send::<OptionalContext<Vec<RpcKeyedAccount>>>(
        RpcRequest::GetProgramAccounts,
        serde_json::json!([crate::drift_cpi::id().to_string(), config]),
      )
      .await?;

    let mut users = vec![];
    if let OptionalContext::Context(accounts) = response {
      users = accounts
        .value
        .into_par_iter()
        .map(|account| {
          let slot = accounts.context.slot;
          Result::<_, anyhow::Error>::Ok(DecodedAcctCtx {
            key: Pubkey::from_str(&account.pubkey)?,
            account: account.account.to_account()?,
            slot,
            decoded: account.account.decode_account::<User>()?,
          })
        })
        .flatten()
        .collect();
    }
    Ok(users)
  }

  pub async fn users_with_order(rpc: &RpcClient) -> anyhow::Result<Vec<DecodedAcctCtx<User>>> {
    let filters = Some(vec![Self::users_with_order_filter()]);
    let account_config = RpcAccountInfoConfig {
      encoding: Some(UiAccountEncoding::Base64),
      ..Default::default()
    };
    let config = RpcProgramAccountsConfig {
      filters,
      account_config,
      with_context: Some(true),
    };

    let response = rpc
      .send::<OptionalContext<Vec<RpcKeyedAccount>>>(
        RpcRequest::GetProgramAccounts,
        serde_json::json!([crate::drift_cpi::id().to_string(), config]),
      )
      .await?;

    let mut users = vec![];
    if let OptionalContext::Context(accounts) = response {
      for account in accounts.value {
        let slot = accounts.context.slot;
        users.push(DecodedAcctCtx {
          key: Pubkey::from_str(&account.pubkey)?,
          account: account.account.to_account()?,
          slot,
          decoded: account.account.decode_account::<User>()?,
        });
      }
    }
    Ok(users)
  }

  pub async fn user_stats(
    rpc: &RpcClient,
    user_auths: &[Pubkey],
  ) -> anyhow::Result<Vec<DecodedAcctCtx<UserStats>>> {
    let pdas = user_auths
      .iter()
      .map(DriftUtils::user_stats_pda)
      .collect::<Vec<Pubkey>>();

    let account_infos = NexusClient::accounts(rpc, &pdas).await?;
    let user_stats: Vec<DecodedAcctCtx<UserStats>> = account_infos
      .into_par_iter()
      .flat_map(|k| {
        match AccountType::decode(k.account.data.as_slice())
          .map_err(|e| anyhow::anyhow!("Failed to decode account: {:?}", e))
        {
          Ok(account) => match account {
            AccountType::UserStats(user) => Some(DecodedAcctCtx {
              key: k.key,
              account: k.account,
              slot: k.slot,
              decoded: user,
            }),
            _ => None,
          },
          Err(e) => {
            log::error!("{:#?}", e);
            None
          }
        }
      })
      .collect();

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
    users
      .into_iter()
      .for_each(|u| match user_auths.get_mut(&u.decoded.authority) {
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
    user_stats
      .into_iter()
      .filter(|us| us.decoded.taker_volume30d > 0 && us.decoded.maker_volume30d > 0)
      .for_each(|us| {
        let users: Vec<DecodedAcctCtx<User>> =
          user_auths.remove(&us.decoded.authority).unwrap_or_default();
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
    let mut trader_stats: Vec<TraderStats> =
      best_traders.into_iter().map(TraderStats::from).collect();
    trader_stats.sort_by_key(|a| a.settled_perp_pnl as i64);
    Ok(trader_stats)
  }

  pub async fn drift_historical_pnl(
    client: &Client,
    user: &Pubkey,
    days_back: i64,
  ) -> anyhow::Result<HistoricalPerformance> {
    let end = Time::now();
    // drift doesn't have anything more recent than 2 days ago
    // let end = end.delta_date(-2);

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

      let fetch = || async {
        client
          .get(url.clone())
          .header("Accept-Encoding", "gzip")
          .send()
          .await
      };

      let mut res = None;
      while res.is_none() {
        match fetch().await {
          Ok(r) => {
            res = Some(r);
          }
          Err(e) => {
            log::error!("Failed to get historical Drift data: {:?}", e);
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
          }
        }
      }
      let res = res.ok_or(anyhow::anyhow!("Failed to get resolve Drift data"))?;

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

  pub async fn perp_market_info(
    cache: &ReadCache<'_>,
    perp_market_index: u16,
  ) -> anyhow::Result<OrderPrice> {
    let market_pda = DriftUtils::perp_market_pda(perp_market_index);
    let perp_market = cache
      .decoded_account::<PerpMarket>(&market_pda, None)?
      .decoded;

    let oracle = perp_market.amm.oracle;
    let oracle_ctx = cache.account(&oracle, None)?;
    let oracle_acct_info =
      oracle_ctx
        .account
        .to_account_info(oracle, false, false, oracle_ctx.account.executable);
    let slot = oracle_ctx.slot;
    let oracle_source = perp_market.amm.oracle_source;
    let price_data = get_oracle_price(&oracle_source, &oracle_acct_info, slot)
      .map_err(|e| anyhow::anyhow!("Failed to get oracle price: {:?}", e))?;
    let price = price_data.price as f64 / PRICE_PRECISION as f64;

    Ok(OrderPrice {
      price,
      name: DriftUtils::decode_name(&perp_market.name),
      offset: 0.0,
    })
  }

  pub fn spot_position_available(pos: &SpotPosition) -> bool {
    pos.scaled_balance == 0 && pos.open_orders == 0
  }

  pub fn perp_position_available(pos: &PerpPosition) -> bool {
    !DriftUtils::perp_is_open_position(pos)
      && !DriftUtils::perp_has_open_order(pos)
      && !DriftUtils::has_unsettled_pnl(pos)
      && !DriftUtils::perp_is_lp(pos)
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
    let spot_market_pdas: Vec<Pubkey> = (0..state.number_of_spot_markets)
      .map(DriftUtils::spot_market_pda)
      .collect();
    let perp_market_pdas: Vec<Pubkey> = (0..state.number_of_markets)
      .map(DriftUtils::perp_market_pda)
      .collect();

    let (spot_markets, perp_markets) = tokio::join!(
      client.get_multiple_accounts(spot_market_pdas.as_slice()),
      client.get_multiple_accounts(perp_market_pdas.as_slice())
    );

    let spot_markets = spot_markets?
      .into_iter()
      .map(|x| {
        let account = x.unwrap();
        SpotMarket::try_deserialize(&mut account.data.as_slice()).unwrap()
      })
      .collect();

    let perp_markets = perp_markets?
      .into_iter()
      .map(|x| {
        let account = x.unwrap();
        PerpMarket::try_deserialize(&mut account.data.as_slice()).unwrap()
      })
      .collect();

    Ok((spot_markets, perp_markets))
  }

  pub fn oracle_price(
    market: &MarketId,
    cache: &ReadCache<'_>,
    slot: Option<u64>,
  ) -> anyhow::Result<f64> {
    match market.kind {
      MarketType::Perp => {
        let market_ctx = cache.decoded_account::<PerpMarket>(&market.key(), slot)?;
        let oracle_ctx = cache.account(&market_ctx.decoded.amm.oracle, slot)?;
        let price_data = get_oracle_price(
          &market_ctx.decoded.amm.oracle_source,
          &oracle_ctx
            .account
            .to_account_info(market_ctx.decoded.amm.oracle, false, false, false),
          oracle_ctx.slot,
        )
        .map_err(|e| anyhow::anyhow!("Failed to get perp oracle price: {:?}", e))?;
        Ok(price_data.price as f64 / PRICE_PRECISION as f64)
      }
      MarketType::Spot => {
        let market_ctx = cache.decoded_account::<SpotMarket>(&market.key(), slot)?;
        let oracle_ctx = cache.account(&market_ctx.decoded.oracle, slot)?;
        let price_data = get_oracle_price(
          &market_ctx.decoded.oracle_source,
          &oracle_ctx
            .account
            .to_account_info(market_ctx.decoded.oracle, false, false, false),
          oracle_ctx.slot,
        )
        .map_err(|e| anyhow::anyhow!("Failed to get spot oracle price: {:?}", e))?;
        Ok(price_data.price as f64 / PRICE_PRECISION as f64)
      }
    }
  }

  // pub fn oracle_price(
  //   src: &OracleSource,
  //   key: Pubkey,
  //   acct: &impl ToAccountInfo,
  //   slot: u64,
  // ) -> anyhow::Result<f64> {
  //   let price_data = get_oracle_price(src, &acct.to_account_info(key, false, false, false), slot)
  //     .map_err(|e| anyhow::anyhow!("Failed to get oracle price: {:?}", e))?;
  //   Ok(price_data.price as f64 / PRICE_PRECISION as f64)
  // }

  pub fn log_order(params: &OrderParams, order_price: &OrderPrice, prefix: Option<&str>) {
    let dir = match params.direction {
      PositionDirection::Long => "long",
      PositionDirection::Short => "short",
    };
    let base = trunc!(params.base_asset_amount as f64 / BASE_PRECISION as f64, 2);
    match prefix {
      Some(prefix) => {
        info!(
          "{}: {} {} {} @ {} as {:?}, offset: {}, price w/o offset?: {}",
          prefix,
          dir,
          base,
          order_price.name,
          trunc!(order_price.price(), 3),
          params.order_type,
          trunc!(order_price.offset, 3),
          trunc!(order_price.price_without_offset(), 3),
        );
      }
      None => {
        info!(
          "{} {} {} @ {} as {:?}, offset: {}, limit?: {}",
          dir,
          base,
          order_price.name,
          trunc!(order_price.price(), 3),
          params.order_type,
          trunc!(order_price.offset, 3),
          trunc!(order_price.price_without_offset(), 3),
        );
      }
    }
  }
}
