#![allow(unused_imports)]

use std::collections::HashMap;

use base64::Engine;
use drift::math::constants::PRICE_PRECISION;
use drift::state::oracle::{get_oracle_price, OraclePriceData};
use solana_sdk::account_info::AccountInfo;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;

pub use client::*;
use common::*;
use decoder::Drift;
pub use trader::*;

pub mod client;
pub mod trader;

/// name: SOL-PERP index: 0
/// name: BTC-PERP index: 1
/// name: ETH-PERP index: 2
/// name: APT-PERP index: 3
/// name: 1MBONK-PERP index: 4
/// name: MATIC-PERP index: 5
/// name: ARB-PERP index: 6
/// name: DOGE-PERP index: 7
/// name: BNB-PERP index: 8
/// name: SUI-PERP index: 9
/// name: 1MPEPE-PERP index: 10
/// name: OP-PERP index: 11
///
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
    oracle: Pubkey,
    oracle_source: drift::state::oracle::OracleSource,
    oracle_price_data: Option<OraclePriceData>,
    perp_name: String,
    perp_market_index: u16,
    spot_market_index: u16,
  }

  let perp_markets = Drift::perp_markets(client.rpc()).await?;
  let spot_markets = Drift::spot_markets(client.rpc()).await?;
  let mut oracles: HashMap<String, MarketInfo> = HashMap::new();
  for market in perp_markets {
    let perp_name = Drift::decode_name(&market.name);
    let spot_market = spot_markets
      .iter()
      .find(|spot| spot.market_index == market.quote_spot_market_index)
      .ok_or(anyhow::anyhow!("Spot market not found"))?;
    let oracle = spot_market.oracle;
    let oracle_source = spot_market.oracle_source;

    oracles.insert(perp_name.clone(), MarketInfo {
      oracle,
      oracle_source,
      oracle_price_data: None,
      perp_name,
      perp_market_index: market.market_index,
      spot_market_index: market.quote_spot_market_index,
    });
  }

  let oracle_keys: Vec<Pubkey> = oracles.values().map(|v| {
    v.oracle
  }).collect();
  let oracle_sources: Vec<drift::state::oracle::OracleSource> = oracles.values().map(|v| {
    v.oracle_source
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
      oracles.get_mut(&name).unwrap().oracle_price_data = Some(price_data);
      let price = price_data.price as f64 / PRICE_PRECISION as f64;

      println!("{}: {}", name, price);
    }
  }

  Ok(())
}


#[tokio::test]
async fn usdc_oracle() -> anyhow::Result<()> {
  init_logger();
  dotenv::dotenv().ok();
  // let rpc_url = "http://localhost:8899".to_string();
  let rpc_url = "https://guillemette-ldmq0k-fast-mainnet.helius-rpc.com/".to_string();
  let signer = ArbiterClient::read_keypair_from_env("WALLET")?;
  let client = ArbiterClient::new(signer, rpc_url).await?;

  let oracle = pubkey!("Gnt27xtC473ZT2Mw5u8wZ68Z3gULkSTb5DuxJy7eJotD");
  let oracle_source = drift::state::oracle::OracleSource::PythStableCoin;
  let res = client.rpc().get_account_with_commitment(&oracle, CommitmentConfig::default()).await?;
  let slot = res.context.slot;
  let raw = res.value.ok_or(anyhow::anyhow!("Failed to get account"))?;

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
  println!("price data: {:#?}", price_data);
  let price = price_data.price as f64 / PRICE_PRECISION as f64;
  println!("price: {}", price);

  Ok(())
}