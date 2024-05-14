use std::collections::HashMap;
use std::time::Instant;
use anchor_lang::Discriminator;
use borsh::BorshDeserialize;
use rayon::prelude::*;
use solana_account_decoder::UiAccountEncoding;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use solana_client::rpc_filter::{Memcmp, RpcFilterType};
use solana_sdk::account::Account;
use solana_sdk::account_info::AccountInfo;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use crate::drift_client::trader::*;
use common::{KeyedAccount, Time, trunc};
use drift_cpi::{AccountType, DiscrimToName, get_oracle_price, PerpMarket, PRICE_PRECISION, SpotBalanceType, SpotMarket, User, UserStats};
use crate::{HistoricalPerformance, HistoricalSettlePnl, MarketInfo, Nexus, ProgramDecoder};

pub const DRIFT_API_PREFIX: &str = "https://drift-historical-data-v2.s3.eu-west-1.amazonaws.com/program/dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH/";

pub struct DriftClient;

impl DriftClient {
  pub fn decode_name(name: &[u8; 32]) -> String {
    String::from_utf8(name.to_vec()).unwrap().trim().to_string()
  }

  pub fn user_pda(authority: &Pubkey, sub_account_id: u16) -> anyhow::Result<Pubkey> {
    let seeds: &[&[u8]] = &[
      b"user",
      &authority.to_bytes()[..],
      &sub_account_id.to_le_bytes(),
    ];
    Ok(Pubkey::find_program_address(seeds, &drift_cpi::id()).0)
  }

  pub fn user_stats_pda(authority: &Pubkey) -> anyhow::Result<Pubkey> {
    let seeds: &[&[u8]] = &[b"user_stats", &authority.to_bytes()[..]];
    Ok(Pubkey::find_program_address(seeds, &drift_cpi::id()).0)
  }

  pub fn spot_market_pda(market_index: u16) -> anyhow::Result<Pubkey> {
    let seeds: &[&[u8]] = &[b"spot_market", &market_index.to_le_bytes()];
    Ok(Pubkey::find_program_address(seeds, &drift_cpi::id()).0)
  }

  pub fn perp_market_pda(market_index: u16) -> anyhow::Result<Pubkey> {
    let seeds: &[&[u8]] = &[b"perp_market", &market_index.to_le_bytes()];
    Ok(Pubkey::find_program_address(seeds, &drift_cpi::id()).0)
  }

  /// token_amount = SpotPosition.scaled_balance as u128
  ///
  /// SpotMarket = fetch SpotMarkets from Epoch and find where
  /// spot_market = SpotMarket.market_index == SpotPosition.market_index
  ///
  /// balance_type = SpotPosition.balance_type
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


  pub async fn perp_markets(client: &RpcClient) -> anyhow::Result<Vec<PerpMarket>> {
    let pdas = (0..50)
      .flat_map(|i| Self::perp_market_pda(i as u16))
      .collect::<Vec<Pubkey>>();

    let keyed_accounts = client.get_multiple_accounts(&pdas).await?;
    let valid_accounts: Vec<Account> = keyed_accounts.into_iter().flatten().collect();
    let markets: Vec<PerpMarket> = valid_accounts
      .into_iter()
      .flat_map(|a| {
        let mut bytes = &a.data.as_slice()[8..];
        match PerpMarket::deserialize(&mut bytes) {
          Ok(market) => Some(market),
          Err(_) => None,
        }
      })
      .collect();
    Ok(markets)
  }

  pub async fn spot_markets(client: &RpcClient) -> anyhow::Result<Vec<SpotMarket>> {
    let pdas = (0..50)
      .flat_map(|i| Self::spot_market_pda(i as u16))
      .collect::<Vec<Pubkey>>();

    let keyed_accounts = client.get_multiple_accounts(&pdas).await?;
    let valid_accounts: Vec<Account> = keyed_accounts.into_iter().flatten().collect();
    let markets: Vec<SpotMarket> = valid_accounts
      .into_iter()
      .flat_map(|a| {
        let mut bytes = &a.data.as_slice()[8..];
        match SpotMarket::deserialize(&mut bytes) {
          Ok(market) => Some(market),
          Err(_) => None,
        }
      })
      .collect();
    Ok(markets)
  }

  pub async fn users(nexus: &Nexus) -> anyhow::Result<Vec<(Pubkey, Account)>> {
    let discrim = User::discriminator();
    let memcmp = Memcmp::new_base58_encoded(0, discrim.to_vec().as_slice());

    let filters = vec![RpcFilterType::Memcmp(memcmp)];

    let account_config = RpcAccountInfoConfig {
      encoding: Some(UiAccountEncoding::Base64),
      ..Default::default()
    };

    let accounts = nexus.rpc
                        .get_program_accounts_with_config(
                          &drift_cpi::id(),
                          RpcProgramAccountsConfig {
                            filters: Some(filters),
                            account_config,
                            ..Default::default()
                          },
                        )
                        .await?;
    Ok(accounts)
  }

  pub async fn user_stats(
    nexus: &Nexus,
    user_auths: &[&Authority],
  ) -> anyhow::Result<Vec<KeyedAccount<UserStats>>> {
    let pdas = user_auths
      .iter()
      .flat_map(|k| Self::user_stats_pda(k))
      .collect::<Vec<Pubkey>>();

    let name = AccountType::discrim_to_name(UserStats::discriminator()).map_err(
      |e| anyhow::anyhow!("Failed to get name for UserStats discrim: {:?}", e)
    )?;
    let account_infos = nexus.accounts(&pdas).await?;
    let user_stats: Vec<KeyedAccount<UserStats>> = account_infos
      .into_par_iter()
      .flat_map(|k| {
        match Nexus::decode_program_account(
          &drift_cpi::id(),
          &name,
          k.account.data.as_slice()
        ) {
          Ok(ProgramDecoder::Drift(account)) => match account {
            AccountType::UserStats(user) => Some(KeyedAccount {
              key: k.key,
              account: user,
            }),
            _ => None,
          }
          Err(_) => None,
        }
      })
      .collect();

    Ok(user_stats)
  }

  /// Gets all Drift users, sorts by highest ROI (perp pnl / deposits), and takes top 1,000 users.
  /// Fetches those 1,000 users' [`UserStats`] accounts to derive "PnL to volume ratio",
  /// and filters out users who have not traded in the last 30 days.
  /// Since one authority can have many User accounts, we map all User accounts to each authority and return.
  pub async fn top_traders(nexus: &Nexus) -> anyhow::Result<HashMap<Authority, DriftTrader>> {
    let start = Instant::now();
    let user_accounts: Vec<(Pubkey, Account)> = Self::users(nexus).await?;
    let end = Instant::now();
    log::info!(
        "Fetched Drift {} users in {}s",
        &user_accounts.len(),
        trunc!(end.duration_since(start).as_secs_f64(), 2)
    );

    // chunk user_accounts into 1000 accounts per chunk
    let chunked_accounts: Vec<_> =
      user_accounts.par_chunks(1_000).collect();

    // par iter over chunked accounts
    let name = AccountType::discrim_to_name(UserStats::discriminator()).map_err(
      |e| anyhow::anyhow!("Failed to get name for UserStats discrim: {:?}", e)
    )?;
    let mut users: Vec<KeyedAccount<User>> = chunked_accounts
      .into_par_iter()
      .flat_map(|chunk| {
        chunk.par_iter().map(|u| {
          match Nexus::decode_program_account(
            &drift_cpi::id(),
            &name,
            u.1.data.as_slice()
          ) {
            Ok(ProgramDecoder::Drift(account)) => match account {
              AccountType::User(user) => Some(KeyedAccount {
                key: u.0,
                account: user,
              }),
              _ => None,
            }
            Err(_) => None,
          }
        })
      }).flatten().collect();

    // sort where highest roi is first index
    users.retain(|u| u.account.total_deposits > 0);
    users.par_sort_by_key(|a| a.account.settled_perp_pnl);

    // let users: Vec<KeyedAccount<User>> = users.into_iter().take(100_000).collect();

    // map all User accounts to each authority
    let mut user_auths = HashMap::<Authority, Vec<KeyedAccount<User>>>::new();
    users
      .into_iter()
      .for_each(|u| match user_auths.get_mut(&u.account.authority) {
        Some(users) => {
          users.push(u);
        }
        None => {
          user_auths.insert(u.account.authority, vec![u]);
        }
      });

    // get UserStats account for each authority
    let auths = user_auths.keys().collect::<Vec<&Authority>>();
    let user_stats = Self::user_stats(nexus, auths.as_slice()).await?;

    // UserStat account is PDA of authority pubkey, so there's only ever 1:1.
    // There is never a case when traders HashMap has an existing entry that needs to be updated.
    // Therefore, insert (which overwrites) is safe.
    let mut traders = HashMap::<Authority, DriftTrader>::new();
    user_stats
      .into_iter()
      // filter traders who have traded in the last 30 days
      .filter(|us| us.account.taker_volume30d > 0 && us.account.maker_volume30d > 0)
      .for_each(|us| {
        let users: Vec<KeyedAccount<User>> =
          user_auths.remove(&us.account.authority).unwrap_or_default();
        let key = us.account.authority;
        let trader = DriftTrader {
          authority: us.account.authority,
          user_stats: us,
          users,
        };
        traders.insert(key, trader);
      });
    Ok(traders)
  }

  /// Top perp traders, sorted by ROI as a ratio of settled perp pnl to total deposits.
  pub async fn top_traders_by_pnl(nexus: &Nexus) -> anyhow::Result<Vec<DriftTrader>> {
    let traders_map = Self::top_traders(nexus).await?;
    let mut traders = traders_map.into_values().collect::<Vec<DriftTrader>>();
    traders.retain(|t| t.settled_perp_pnl() > 0_f64);
    traders.sort_by_key(|a| a.settled_perp_pnl() as i64);
    Ok(traders)
  }

  /// Formatted into [`TraderStats`] struct for easy display and less memory usage.
  pub async fn top_trader_stats_by_pnl(nexus: &Nexus) -> anyhow::Result<Vec<TraderStats>> {
    let best_traders = Self::top_traders_by_pnl(nexus).await?;
    let mut trader_stats: Vec<TraderStats> =
      best_traders.into_iter().map(TraderStats::from).collect();
    trader_stats.sort_by_key(|a| a.settled_perp_pnl as i64);
    Ok(trader_stats)
  }

  pub async fn drift_historical_pnl(
    nexus: &Nexus,
    user: &Pubkey,
    days_back: i64
  ) -> anyhow::Result<HistoricalPerformance> {
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

      let res = nexus.client
                     .get(url.clone())
        // gzip header
                     .header("Accept-Encoding", "gzip")
                     .send()
                     .await?;
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

  pub async fn perp_market_info(rpc: &RpcClient, perp_market_index: u16) -> anyhow::Result<MarketInfo> {
    let market_pda = DriftClient::perp_market_pda(perp_market_index)?;
    let market_acct = rpc.get_account(&market_pda).await?;
    let mut bytes = &market_acct.data.as_slice()[8..];
    let perp_market = match PerpMarket::deserialize(&mut bytes) {
      Ok(market) => Some(market),
      Err(_) => None,
    }.ok_or(anyhow::anyhow!("Failed to deserialize perp market"))?;

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
      slot
    ).map_err(|e| anyhow::anyhow!("Failed to get oracle price: {:?}", e))?;
    let price = price_data.price as f64 / PRICE_PRECISION as f64;

    Ok(MarketInfo {
      price,
      name: DriftClient::decode_name(&perp_market.name),
    })
  }
}