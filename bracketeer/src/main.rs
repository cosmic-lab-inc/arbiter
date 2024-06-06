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

use bracketeer::*;
use nexus::drift_client::*;
use nexus::drift_cpi::*;
use nexus::*;

mod bracketeer;
mod config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  dotenv::dotenv().ok();
  init_logger();

  // SOL-PERP
  let market = MarketId::perp(0);
  let mut baker = Bracketeer::new(0, market, None).await?;
  baker.start().await?;

  Ok(())
}

// #[cfg(tests)]
mod tests {
  use super::*;
  use solana_client::nonblocking::rpc_client::RpcClient;
  use std::collections::HashSet;
  use yellowstone_grpc_proto::prelude::{
    CommitmentLevel, SubscribeRequestFilterAccounts, SubscribeRequestFilterSlots,
  };

  #[tokio::test]
  async fn bid_ask_prices() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    init_logger();

    // SOL-PERP
    let market = MarketId::perp(0);
    let baker = Bracketeer::new(0, market, None).await?;

    let now = std::time::Instant::now();
    while now.elapsed() < std::time::Duration::from_secs(30) {
      let res = baker
        .drift
        .bid_ask_prices(market, true, &baker.cache().await)?;
      let BidAsk { bid, ask, .. } = res;

      let market_info = baker
        .drift
        .market_info(market, &baker.cache().await, None)?;
      let oracle = market_info.price;

      info!(
        "bid: {}, ask: {}, oracle: {}, spread: {}, mark: {}",
        trunc!(bid, 10),
        trunc!(ask, 10),
        trunc!(oracle, 10),
        trunc!(res.spread(), 10),
        trunc!(res.mark(), 10)
      );

      tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    Ok(())
  }

  #[tokio::test]
  async fn orderbook() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    init_logger();

    let market_id = MarketId::perp(0);
    let cache = Cache::new(200);
    let orderbook = Orderbook::new(vec![market_id]);
    let (tx, _rx) = crossbeam::channel::unbounded::<TxStub>();
    let grpc = std::env::var("GRPC")?;
    let x_token = std::env::var("X_TOKEN")?;
    let signer = read_keypair_from_env("WALLET")?;
    let rpc_url = std::env::var("RPC_URL")?;
    let rpc = RpcClient::new(rpc_url);

    // accounts to subscribe to
    let now = std::time::Instant::now();
    let perps = DriftUtils::perp_markets(&rpc).await?;
    let spots = DriftUtils::spot_markets(&rpc).await?;
    let perp_markets: Vec<Pubkey> = perps.iter().map(|p| p.key).collect();
    let spot_markets: Vec<Pubkey> = spots.iter().map(|s| s.key).collect();
    let user = DriftUtils::user_pda(&signer.pubkey(), 0);
    let select_users = [user];
    let users = DriftUtils::users(&rpc).await?;
    let user_keys: Vec<Pubkey> = users.iter().map(|ctx| ctx.key).collect();
    let perp_oracles: Vec<Pubkey> = perps.iter().map(|p| p.decoded.amm.oracle).collect();
    let spot_oracles: Vec<Pubkey> = spots.iter().map(|s| s.decoded.oracle).collect();
    info!("time to load filters: {:?}", now.elapsed());
    let accounts = perp_markets
      .iter()
      .chain(spot_markets.iter())
      .chain(user_keys.iter())
      .chain(perp_oracles.iter())
      .chain(spot_oracles.iter())
      .cloned()
      .collect::<Vec<Pubkey>>();
    let mut filter = HashSet::new();
    for a in accounts {
      filter.insert(a);
    }

    let now = std::time::Instant::now();
    let auths = [signer.pubkey()];
    cache
      .write()
      .await
      .load(&rpc, &select_users, None, &auths)
      .await?;
    info!("time to load cache: {:?}", now.elapsed());
    let now = std::time::Instant::now();
    orderbook.write().await.load(users)?;
    info!("time to load orderbook: {:?}", now.elapsed());

    let cfg = GeyserConfig {
      grpc,
      x_token,
      slots: Some(SubscribeRequestFilterSlots {
        filter_by_commitment: Some(true),
      }),
      accounts: Some(SubscribeRequestFilterAccounts {
        account: vec![],
        owner: vec![drift_cpi::id().to_string(), PYTH_PROGRAM_ID.to_string()],
        filters: vec![],
      }),
      transactions: None,
      blocks_meta: None,
      commitment: CommitmentLevel::Processed,
    };
    // stream updates from gRPC
    let nexus = NexusClient::new(cfg)?;

    let _orderbook = orderbook.clone();
    let _cache = cache.clone();
    tokio::task::spawn(async move {
      nexus
        .stream(&_cache, Some(tx), Some(&_orderbook), Some(filter))
        .await?;
      Result::<_, anyhow::Error>::Ok(())
    });

    let orderbook = orderbook.clone();
    let cache = cache.clone();
    let now = std::time::Instant::now();
    while now.elapsed() < std::time::Duration::from_secs(60 * 10) {
      let dlob = orderbook.read().await;
      let cache = cache.read().await;
      let price = DriftUtils::oracle_price(&market_id, &cache, None)?;

      let l3 = dlob.l3(&market_id, &cache)?;
      let orders = dlob.market_orders(&market_id)?;
      drop(cache);
      drop(dlob);
      let quote_spread = trunc!(l3.spread, 4);
      let pct_spread = trunc!(l3.spread / price * 100.0, 3);
      info!(
        "price: {}, bid: {}, ask: {}, spread: ${}, spread: {}%, orders: {}",
        trunc!(price, 4),
        trunc!(l3.best_bid()?.price, 4),
        trunc!(l3.best_ask()?.price, 4),
        quote_spread,
        pct_spread,
        orders
      );
      tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
    Ok(())
  }
}
