#![allow(unused_imports)]
#![allow(dead_code)]

use std::collections::HashMap;

use anchor_lang::Discriminator;
use anchor_lang::prelude::AccountInfo;
use base64::Engine;
use rayon::prelude::*;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;

pub use client::*;
use common::*;
use decoder::{Decoder, PnlStub};
use decoder::drift::DriftClient;
use drift_cpi::math::PRICE_PRECISION;
use drift_cpi::oracle::{get_oracle_price, OraclePriceData};
use drift_cpi::OracleSource;
pub use trader::*;
pub use time::*;

pub mod client;
pub mod trader;
pub mod time;

/// NodeJS websocket: https://github.com/drift-labs/protocol-v2/blob/ebe773e4594bccc44e815b4e45ed3b6860ac2c4d/sdk/src/accounts/webSocketAccountSubscriber.ts#L174
/// Rust websocket: https://github.com/drift-labs/drift-rs/blob/main/src/websocket_program_account_subscriber.rs
/// Rust oracle type: https://github.com/drift-labs/protocol-v2/blob/ebe773e4594bccc44e815b4e45ed3b6860ac2c4d/programs/drift/src/state/oracle.rs#L126
/// Pyth deser: https://github.com/pyth-network/pyth-sdk-rs/blob/main/pyth-sdk-solana/examples/get_accounts.rs#L67
#[tokio::test]
async fn drift_perp_markets() -> anyhow::Result<()> {
  init_logger();
  dotenv::dotenv().ok();
  // let rpc_url = "http://localhost:8899".to_string();
  let rpc_url = "https://guillemette-ldmq0k-fast-mainnet.helius-rpc.com/".to_string();
  let signer = ArbiterClient::read_keypair_from_env("WALLET")?;
  let client = ArbiterClient::new(signer, rpc_url).await?;

  struct MarketInfo {
    perp_oracle: Pubkey,
    perp_oracle_source: OracleSource,
    perp_oracle_price_data: Option<OraclePriceData>,
    spot_oracle: Pubkey,
    spot_oracle_source: OracleSource,
    spot_oracle_price_data: Option<OraclePriceData>,
    perp_name: String,
    perp_market_index: u16,
    spot_market_index: u16,
  }

  let perp_markets = DriftClient::perp_markets(client.rpc()).await?;
  let spot_markets = DriftClient::spot_markets(client.rpc()).await?;
  let mut oracles: HashMap<String, MarketInfo> = HashMap::new();
  for market in perp_markets {
    let perp_name = DriftClient::decode_name(&market.name);
    let spot_market = spot_markets
      .iter()
      .find(|spot| spot.market_index == market.quote_spot_market_index)
      .ok_or(anyhow::anyhow!("Spot market not found"))?;
    let spot_oracle = spot_market.oracle;
    let spot_oracle_source = spot_market.oracle_source;
    let perp_oracle = market.amm.oracle;
    let perp_oracle_source = market.amm.oracle_source;

    oracles.insert(perp_name.clone(), MarketInfo {
      perp_oracle,
      perp_oracle_source,
      perp_oracle_price_data: None,
      spot_oracle,
      spot_oracle_source,
      spot_oracle_price_data: None,
      perp_name,
      perp_market_index: market.market_index,
      spot_market_index: market.quote_spot_market_index,
    });
  }

  let oracle_keys: Vec<Pubkey> = oracles.values().map(|v| {
    v.perp_oracle
  }).collect();
  let oracle_sources: Vec<OracleSource> = oracles.values().map(|v| {
    v.perp_oracle_source
  }).collect();
  let names: Vec<String> = oracles.keys().cloned().collect();


  let res = client.rpc().get_multiple_accounts_with_commitment(oracle_keys.as_slice(), CommitmentConfig::default()).await?;
  let slot = res.context.slot;

  for (i, v) in res.value.into_iter().enumerate() {
    if let Some(raw) = v {
      let name = names[i].clone();
      let oracle = oracle_keys[i];
      let oracle_source = oracle_sources[i];
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
      let price_data = get_oracle_price(
        &oracle_source,
        &oracle_acct_info,
        slot
      ).map_err(|e| anyhow::anyhow!("Failed to get oracle price: {:?}", e))?;
      oracles.get_mut(&name).unwrap().perp_oracle_price_data = Some(price_data);
      let price = price_data.price as f64 / PRICE_PRECISION as f64;

      println!("{}: {}", name, price);
    }
  }

  Ok(())
}


/// cargo test --package arbiter_client --lib top_users -- --exact --show-output
#[tokio::test]
async fn top_users() -> anyhow::Result<()> {
  init_logger();
  dotenv::dotenv().ok();
  let rpc_url = "https://rpc.hellomoon.io/250fbc17-3f01-436a-b6dd-993e8e32a47d".to_string();
  let signer = ArbiterClient::read_keypair_from_env("WALLET")?;
  let client = ArbiterClient::new(signer, rpc_url).await?;
  let decoder = Decoder::new()?;

  let users = DriftClient::top_traders_by_pnl(client.rpc(), &decoder).await?;
  println!("users: {}", users.len());
  let stats: Vec<decoder::TraderStub> = users.into_iter().map(|u| {
    decoder::TraderStub {
      authority: u.authority.to_string(),
      pnl: u.settled_perp_pnl(),
      best_user: u.best_user().key.to_string(),
    }
  }).collect();
  // write to json file
  let stats_json = serde_json::to_string(&stats)?;
  std::fs::write("top_traders.json", stats_json)?;
  Ok(())
}

/// cargo test --package arbiter_client --lib historical_pnl -- --exact --show-output
#[tokio::test]
async fn historical_pnl() -> anyhow::Result<()> {
  init_logger();
  dotenv::dotenv().ok();
  let rpc_url = "https://rpc.hellomoon.io/250fbc17-3f01-436a-b6dd-993e8e32a47d".to_string();
  let signer = ArbiterClient::read_keypair_from_env("WALLET")?;
  let client = ArbiterClient::new(signer, rpc_url).await?;

  let user = pubkey!("4oTeSjNig62yD4KCehU4jkpNVYowLfaTie6LTtGbmefX");

  let data = client.drift_historical_pnl(
    &user,
    5
  ).await?;

  let data: Vec<PnlStub> = data.into_iter().map(|d| {
    PnlStub {
      pnl: d.pnl,
      user: d.user.to_string(),
      ts: d.ts,
    }
  }).collect();

  // write to json file
  let json = serde_json::to_string(&data)?;
  std::fs::write("pnl_history.json", json)?;


  Ok(())
}