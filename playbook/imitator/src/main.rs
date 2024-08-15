#![allow(unused_imports)]
#![allow(dead_code)]

use std::collections::HashMap;
use std::str::FromStr;

use anchor_lang::prelude::AccountInfo;
use anchor_lang::{AccountDeserialize, Discriminator};
use base64::Engine;
use borsh::BorshDeserialize;
use futures::{SinkExt, StreamExt};
use heck::ToPascalCase;
use log::info;
use rayon::prelude::*;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::Signer;

use client::*;
use nexus::drift_client::*;
use nexus::drift_cpi::*;
use nexus::*;

mod client;
mod config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  dotenv::dotenv().ok();
  init_logger();

  // let copy_user = pubkey!("H5jfagEnMVNH3PMc2TU2F7tNuXE6b4zCwoL5ip1b4ZHi");
  // let copy_user = pubkey!("772JJ15pJV64Uit5BmV4CoJtMQHx9KRdHhizP7mAgjRQ");

  // 5 brackets orders, starting at 0.08% spread and incrementing by 0.1%
  // self trades if one or more orders are filled and can match against its own MM orders.
  let copy_user = pubkey!("2aMcirYcF9W8aTFem6qe8QtvfQ22SLY6KUe6yUQbqfHk");

  let market_filter = Some(vec![
    // SOL-PERP
    MarketId::perp(0),
  ]);
  let mut imitator = Imitator::new(0, copy_user, market_filter, None).await?;
  imitator.start().await?;

  Ok(())
}

// ============================================================================
// Tests
// ============================================================================

// #[cfg(tests)]
mod tests {
  use super::*;
  use solana_client::nonblocking::rpc_client::RpcClient;
  use solana_sdk::sysvar::SysvarId;
  use std::collections::HashSet;
  use std::sync::Arc;
  use tokio::sync::RwLock;
  use yellowstone_grpc_proto::prelude::{
    CommitmentLevel, SubscribeRequestFilterAccounts, SubscribeRequestFilterSlots,
  };

  /// Rust websocket: https://github.com/drift-labs/drift-rs/blob/main/src/websocket_program_account_subscriber.rs
  /// Rust oracle type: https://github.com/drift-labs/protocol-v2/blob/ebe773e4594bccc44e815b4e45ed3b6860ac2c4d/programs/drift/src/state/oracle.rs#L126
  /// Pyth deser: https://github.com/pyth-network/pyth-sdk-rs/blob/main/pyth-sdk-solana/examples/get_accounts.rs#L67
  #[tokio::test]
  async fn drift_perp_markets() -> anyhow::Result<()> {
    init_logger();
    dotenv::dotenv().ok();

    let copy_user = pubkey!("H5jfagEnMVNH3PMc2TU2F7tNuXE6b4zCwoL5ip1b4ZHi");
    let market_filter = Some(vec![
      // SOL-PERP
      MarketId::perp(0),
    ]);
    let imitator = Imitator::new(0, copy_user, market_filter, None).await?;

    let perp_markets = DriftUtils::perp_markets(&imitator.rpc()).await?;
    let spot_markets = DriftUtils::spot_markets(&imitator.rpc()).await?;
    let mut oracles: HashMap<String, MarketMetadata> = HashMap::new();
    for acct in perp_markets {
      let DecodedAcctCtx {
        decoded: market, ..
      } = acct;
      let perp_name = DriftUtils::decode_name(&market.name);
      let spot_market = spot_markets
        .iter()
        .find(|spot| spot.decoded.market_index == market.quote_spot_market_index)
        .ok_or(anyhow::anyhow!("Spot market not found"))?;
      let spot_oracle = spot_market.decoded.oracle;
      let spot_oracle_source = spot_market.decoded.oracle_source;
      let perp_oracle = market.amm.oracle;
      let perp_oracle_source = market.amm.oracle_source;

      oracles.insert(
        perp_name.clone(),
        MarketMetadata {
          perp_oracle,
          perp_oracle_source,
          perp_oracle_price_data: None,
          spot_oracle,
          spot_oracle_source,
          spot_oracle_price_data: None,
          perp_name,
          perp_market_index: market.market_index,
          spot_market_index: market.quote_spot_market_index,
        },
      );
    }

    let oracle_keys: Vec<Pubkey> = oracles.values().map(|v| v.perp_oracle).collect();
    let oracle_sources: Vec<OracleSource> =
      oracles.values().map(|v| v.perp_oracle_source).collect();
    let names: Vec<String> = oracles.keys().cloned().collect();
    let perp_indexes: Vec<u16> = oracles.values().map(|v| v.perp_market_index).collect();

    let res = imitator
      .rpc()
      .get_multiple_accounts_with_commitment(oracle_keys.as_slice(), CommitmentConfig::default())
      .await?;
    let slot = res.context.slot;

    for (i, v) in res.value.into_iter().enumerate() {
      if let Some(raw) = v {
        let name = names[i].clone();
        let oracle = oracle_keys[i];
        let oracle_source = oracle_sources[i];
        let perp_index = perp_indexes[i];
        let mut data = raw.data;
        let mut lamports = raw.lamports;
        let oracle_acct_info = AccountInfo::new(
          &oracle,
          false,
          false,
          &mut lamports,
          &mut data,
          &raw.owner,
          raw.executable,
          raw.rent_epoch,
        );
        // println!("perp oracle program: {}", raw.owner);
        let price_data = get_oracle_price(&oracle_source, &oracle_acct_info, slot)
          .map_err(|e| anyhow::anyhow!("Failed to get oracle price: {:?}", e))?;
        oracles.get_mut(&name).unwrap().perp_oracle_price_data = Some(price_data);
        let price = price_data.price as f64 / PRICE_PRECISION as f64;

        println!("{} ({}): {}", name, perp_index, price);
      }
    }

    Ok(())
  }

  #[tokio::test]
  async fn drift_spot_markets() -> anyhow::Result<()> {
    init_logger();
    dotenv::dotenv().ok();

    let copy_user = pubkey!("H5jfagEnMVNH3PMc2TU2F7tNuXE6b4zCwoL5ip1b4ZHi");
    let market_filter = Some(vec![
      // SOL-PERP
      MarketId::perp(0),
    ]);
    let imitator = Imitator::new(0, copy_user, market_filter, None).await?;

    struct MarketMetadata {
      name: String,
      oracle: Pubkey,
      oracle_source: OracleSource,
      oracle_price_data: Option<OraclePriceData>,
      market_index: u16,
      mint: Pubkey,
    }

    let spot_markets = DriftUtils::spot_markets(&imitator.rpc()).await?;
    let mut oracles: Vec<MarketMetadata> = vec![];
    for spot_market in spot_markets {
      let name = DriftUtils::decode_name(&spot_market.decoded.name);
      let oracle = spot_market.decoded.oracle;
      let oracle_source = spot_market.decoded.oracle_source;

      oracles.push(MarketMetadata {
        name,
        oracle,
        oracle_source,
        oracle_price_data: None,
        market_index: spot_market.decoded.market_index,
        mint: spot_market.decoded.mint,
      });
    }

    let oracle_keys: Vec<Pubkey> = oracles.iter().map(|v| v.oracle).collect();

    let res = imitator
      .rpc()
      .get_multiple_accounts_with_commitment(oracle_keys.as_slice(), CommitmentConfig::default())
      .await?;
    let slot = res.context.slot;

    for (oracle_acct, market_info) in res.value.into_iter().zip(oracles.iter()) {
      if let Some(raw) = oracle_acct {
        let mut data = raw.data;
        let mut lamports = raw.lamports;
        let oracle_acct_info = AccountInfo::new(
          &market_info.oracle,
          false,
          false,
          &mut lamports,
          &mut data,
          &raw.owner,
          raw.executable,
          raw.rent_epoch,
        );
        println!("spot oracle program: {}", raw.owner);
        let price_data = get_oracle_price(&market_info.oracle_source, &oracle_acct_info, slot)
          .map_err(|e| anyhow::anyhow!("Failed to get oracle price: {:?}", e))?;
        let price = price_data.price as f64 / PRICE_PRECISION as f64;

        println!(
          "{} ({}): {}, mint: {}",
          market_info.name, market_info.market_index, price, market_info.mint
        );
      }
    }

    Ok(())
  }

  /// cargo test --package imitator --bin imitator top_users -- --exact --show-output
  #[tokio::test]
  async fn top_users() -> anyhow::Result<()> {
    init_logger();
    dotenv::dotenv().ok();

    let copy_user = pubkey!("H5jfagEnMVNH3PMc2TU2F7tNuXE6b4zCwoL5ip1b4ZHi");
    let market_filter = Some(vec![
      // SOL-PERP
      MarketId::perp(0),
    ]);
    let imitator = Imitator::new(0, copy_user, market_filter, None).await?;

    let users = DriftUtils::top_traders_by_pnl(&imitator.rpc()).await?;
    println!("users: {}", users.len());
    let stats: Vec<TraderStub> = users
      .into_iter()
      .map(|u| TraderStub {
        authority: u.authority.to_string(),
        pnl: u.settled_perp_pnl(),
        best_user: u.best_user().key.to_string(),
      })
      .collect();
    // write to json file
    let stats_json = serde_json::to_string(&stats)?;
    std::fs::write("traders.json", stats_json)?;
    Ok(())
  }

  /// cargo test --package imitator --bin imitator my_pnl -- --exact --show-output
  #[tokio::test]
  async fn my_pnl() -> anyhow::Result<()> {
    init_logger();
    dotenv::dotenv().ok();

    // let copy_user = pubkey!("H5jfagEnMVNH3PMc2TU2F7tNuXE6b4zCwoL5ip1b4ZHi");
    let copy_user = pubkey!("EWvvvhYYKnRAyaw5PGpPRs4TkdAVMw1vL5ZvYFSzNcSQ");
    let market_filter = Some(vec![
      // SOL-PERP
      MarketId::perp(0),
    ]);
    let imitator = Imitator::new(0, copy_user, market_filter, None).await?;

    let prefix = env!("CARGO_MANIFEST_DIR").to_string();

    let user = imitator.user();
    let data = DriftUtils::drift_historical_pnl(&imitator.client(), user, 10).await?;

    if data.dataset().len() > 1 {
      Plot::plot_without_legend(
        vec![data.dataset()],
        &format!("{}/my_cum_pnl.png", prefix),
        &format!("{} Performance", user),
        "Cum USDC PnL",
        "Unix Seconds",
      )?;
      info!("{} done", shorten_address(user));
    }

    Ok(())
  }

  /// cargo test --package imitator --bin imitator historical_pnl -- --exact --show-output
  #[tokio::test]
  async fn historical_pnl() -> anyhow::Result<()> {
    init_logger();
    dotenv::dotenv().ok();

    let copy_user = pubkey!("H5jfagEnMVNH3PMc2TU2F7tNuXE6b4zCwoL5ip1b4ZHi");
    let market_filter = Some(vec![
      // SOL-PERP
      MarketId::perp(0),
    ]);
    let imitator = Imitator::new(0, copy_user, market_filter, None).await?;

    let prefix = env!("CARGO_MANIFEST_DIR").to_string();

    let path = format!("{}/traders.json", prefix);
    let top_traders: Vec<TraderStub> = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    // ordered least to greatest profit, so reverse order and take the best performers
    // todo: run this
    let top_traders: Vec<TraderStub> = top_traders.into_iter().rev().take(175).collect();
    let top_traders = top_traders.as_slice()[125..].to_vec();

    let users: Vec<Pubkey> = top_traders
      .into_iter()
      .flat_map(|t| Pubkey::from_str(&t.best_user))
      .collect();

    let mut top_dogs = vec![];
    for user in users {
      let data = DriftUtils::drift_historical_pnl(&imitator.client(), &user, 100).await?;
      tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

      if data.dataset().len() > 1 && data.avg_quote_pnl() > 0.0 {
        top_dogs.push(data.clone());

        // let json = serde_json::to_string(data.dataset().as_slice())?;
        // std::fs::write(&format!("{}/data/{}.json", prefix, user), json)?;

        Plot::plot_without_legend(
          vec![data.dataset()],
          &format!("{}/images/{}_cum_pnl.png", prefix, user),
          &format!("{} Performance", user),
          "Cum USDC PnL",
          "Unix Seconds",
        )?;
        info!("{} done", shorten_address(&user));
      }
    }

    // todo: sort by linear regression of pnl and/or max drawdown instead
    let best: Vec<PnlStub> = top_dogs
      .into_iter()
      .map(|d| PnlStub {
        user: d.user(),
        avg_quote_pnl: d.avg_quote_pnl(),
      })
      .collect();
    let json = serde_json::to_string(&best)?;
    std::fs::write("top_traders.json", json)?;

    Ok(())
  }

  #[tokio::test]
  async fn account() -> anyhow::Result<()> {
    init_logger();
    dotenv::dotenv().ok();

    let copy_user = pubkey!("H5jfagEnMVNH3PMc2TU2F7tNuXE6b4zCwoL5ip1b4ZHi");
    let market_filter = Some(vec![
      // SOL-PERP
      MarketId::perp(0),
    ]);
    let imitator = Imitator::new(0, copy_user, market_filter, None).await?;
    let key = imitator.signer.pubkey();
    let acct = imitator.rpc().get_account(&key).await?;
    println!("{:#?}", acct);

    Ok(())
  }

  #[tokio::test]
  async fn spot_balance() -> anyhow::Result<()> {
    init_logger();
    dotenv::dotenv().ok();

    let copy_user = pubkey!("H5jfagEnMVNH3PMc2TU2F7tNuXE6b4zCwoL5ip1b4ZHi");
    let market_filter = Some(vec![
      // SOL-PERP
      MarketId::perp(0),
    ]);
    let imitator = Imitator::new(0, copy_user, market_filter, None).await?;
    let user_key = DriftUtils::user_pda(&imitator.signer.pubkey(), 0);
    let user_acct = imitator.rpc().get_account(&user_key).await?;
    let user = User::deserialize(&mut &user_acct.data.as_slice()[8..])?;

    let pm_key = DriftUtils::perp_market_pda(SOL_PERP_MARKET_INDEX);
    let pm_acct = imitator.rpc().get_account(&pm_key).await?;
    let pm = PerpMarket::deserialize(&mut &pm_acct.data.as_slice()[8..])?;

    let sm_key = DriftUtils::spot_market_pda(pm.quote_spot_market_index);
    let sm_acct = imitator.rpc().get_account(&sm_key).await?;
    let sm = SpotMarket::deserialize(&mut &sm_acct.data.as_slice()[8..])?;

    let spot_pos = user
      .spot_positions
      .iter()
      .find(|p| p.market_index == sm.market_index)
      .ok_or(anyhow::anyhow!("User has no position in spot market"))?;
    let quote_amt = DriftUtils::spot_balance(
      spot_pos.cumulative_deposits as u128,
      &sm,
      &spot_pos.balance_type,
      false,
    )?
    .balance;
    println!("cum deposits: {}", spot_pos.cumulative_deposits);
    println!("quote amount: {}", quote_amt);

    Ok(())
  }

  #[tokio::test]
  async fn graphql() -> anyhow::Result<()> {
    init_logger();
    dotenv::dotenv().ok();

    let url = std::env::var("GRAPHQL")?;
    let client = GraphqlClient::new(url);
    client.drift_users().await?;
    // info!("graphql users: {}", users.len());

    Ok(())
  }

  /// cargo test --package imitator --bin imitator trader_pnl -- --exact --show-output
  #[tokio::test]
  async fn trader_pnl() -> anyhow::Result<()> {
    init_logger();
    dotenv::dotenv().ok();

    // Circuit Trading "Supercharger Vault"
    // https://app.drift.trade/SOL-PERP?userAccount=BRksHqLiq2gvQw1XxsZq6DXZjD3GB5a9J63tUBgd6QS9
    let user = pubkey!("BRksHqLiq2gvQw1XxsZq6DXZjD3GB5a9J63tUBgd6QS9");
    let market_filter = Some(vec![MarketId::SOL_PERP]);
    let imitator = Imitator::new(0, user, market_filter, None).await?;

    let data = DriftUtils::drift_historical_pnl(&imitator.client(), &user, 100).await?;

    let prefix = env!("CARGO_MANIFEST_DIR").to_string();
    if data.dataset().len() > 1 {
      Plot::plot_without_legend(
        vec![data.dataset()],
        &format!("{}/{}_pnl.png", prefix, user),
        &format!("{} Performance", user),
        "Cum USDC PnL",
        "Unix Seconds",
      )?;
      info!("{} done", shorten_address(&user));
    }

    Ok(())
  }
}
