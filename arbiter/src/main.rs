#![allow(unused_imports)]
#![allow(dead_code)]

use std::collections::HashMap;
use std::str::FromStr;

use anchor_lang::{account, Discriminator};
use anchor_lang::prelude::AccountInfo;
use base64::Engine;
use base64::engine::general_purpose;
use borsh::BorshDeserialize;
use futures::StreamExt;
use rayon::prelude::*;
use solana_client::nonblocking::pubsub_client::PubsubClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::{bs58, pubkey};
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status::{EncodedTransaction, UiInstruction, UiMessage, UiParsedInstruction};

pub use client::*;
use common::*;
use nexus::drift_cpi::{Decode, InstructionType, PositionDirection, PRICE_PRECISION, get_oracle_price, OraclePriceData, OracleSource, User, PerpMarket, DiscrimToName, NameToDiscrim, OrderType, BASE_PRECISION};
use nexus::{PnlStub, TradeRecord, DriftClient, MarketInfo};
pub use time::*;
use nexus::Nexus;
use heck::ToPascalCase;
use crate::cache::AccountCache;
use crate::types::ChannelEvent;

mod client;
mod types;
mod cache;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  init_logger();
  dotenv::dotenv().ok();

  let arbiter = Arbiter::new_from_env().await?;

  arbiter.stream_accounts().await?;

  loop {}

  // let key = pubkey!("H5jfagEnMVNH3PMc2TU2F7tNuXE6b4zCwoL5ip1b4ZHi");
  // let (mut stream, _unsub) = arbiter.nexus.stream_transactions(&key).await?;
  //
  // while let Some(event) = stream.next().await {
  //   match event.transaction.transaction {
  //     EncodedTransaction::Binary(data, encoding) => {
  //       if encoding == solana_transaction_status::TransactionBinaryEncoding::Base64 {
  //         let bytes = general_purpose::STANDARD.decode(data)?;
  //         let _name = InstructionType::discrim_to_name(bytes[..8].try_into()?).map_err(
  //           |e| anyhow::anyhow!("Failed to get name for instruction: {:?}", e)
  //         )?;
  //       }
  //     }
  //     EncodedTransaction::Json(tx) => {
  //       if let UiMessage::Parsed(msg) = tx.message {
  //         for ui_ix in msg.instructions {
  //           if let UiInstruction::Parsed(ui_parsed_ix) = ui_ix {
  //             match ui_parsed_ix {
  //               UiParsedInstruction::Parsed(parsed_ix) => {
  //                 log::debug!("parsed ix for program \"{}\": {:#?}", parsed_ix.program, parsed_ix.parsed)
  //               }
  //               UiParsedInstruction::PartiallyDecoded(ui_decoded_ix) => {
  //                 let data: Vec<u8> = bs58::decode(ui_decoded_ix.data.clone()).into_vec()?;
  //                 if data.len() >= 8 && ui_decoded_ix.program_id == nexus::drift_cpi::id().to_string() {
  //                   if let Ok(discrim) = data[..8].try_into() {
  //                     let ix = InstructionType::decode(&data[..]).map_err(
  //                       |e| anyhow::anyhow!("Failed to decode instruction: {:?}", e)
  //                     )?;
  //                     let name = InstructionType::discrim_to_name(discrim).unwrap();
  //                     match ix {
  //                       InstructionType::PlacePerpOrder(ix) => {
  //                         let params = ix._params;
  //                         let market_info = DriftClient::perp_market_info(arbiter.rpc(), params.market_index).await?;
  //                         arbiter.log_order(&name, &params, &market_info);
  //                         log::debug!("params: {:#?}", params);
  //                       }
  //                       InstructionType::PlaceAndTakePerpOrder(ix) => {
  //                         let params = ix._params;
  //                         let market_info = DriftClient::perp_market_info(arbiter.rpc(), params.market_index).await?;
  //                         arbiter.log_order(&name, &params, &market_info);
  //                         log::debug!("params: {:#?}", params);
  //                       }
  //                       InstructionType::PlaceOrders(ix) => {
  //                         for params in ix._params {
  //                           let market_info = DriftClient::perp_market_info(arbiter.rpc(), params.market_index).await?;
  //                           arbiter.log_order(&name, &params, &market_info);
  //                           log::debug!("params: {:#?}", params);
  //                         }
  //                       }
  //                       _ => {}
  //                     }
  //                   }
  //                 }
  //               }
  //             }
  //           }
  //         }
  //       }
  //     }
  //     _ => {}
  //   }
  // }

  Ok(())
}

/// NodeJS websocket: https://github.com/drift-labs/protocol-v2/blob/ebe773e4594bccc44e815b4e45ed3b6860ac2c4d/sdk/src/accounts/webSocketAccountSubscriber.ts#L174
/// Rust websocket: https://github.com/drift-labs/drift-rs/blob/main/src/websocket_program_account_subscriber.rs
/// Rust oracle type: https://github.com/drift-labs/protocol-v2/blob/ebe773e4594bccc44e815b4e45ed3b6860ac2c4d/programs/drift/src/state/oracle.rs#L126
/// Pyth deser: https://github.com/pyth-network/pyth-sdk-rs/blob/main/pyth-sdk-solana/examples/get_accounts.rs#L67
#[tokio::test]
async fn drift_perp_markets() -> anyhow::Result<()> {
  init_logger();
  dotenv::dotenv().ok();

  let arbiter = Arbiter::new_from_env().await?;

  let perp_markets = DriftClient::perp_markets(arbiter.rpc()).await?;
  let spot_markets = DriftClient::spot_markets(arbiter.rpc()).await?;
  let mut oracles: HashMap<String, MarketInfo> = HashMap::new();
  for acct in perp_markets {
    let DecodedAccountContext {
      key,
      decoded: market,
      ..
    } = acct;
    let perp_name = DriftClient::decode_name(&market.name);
    let spot_market = spot_markets
      .iter()
      .find(|spot| spot.decoded.market_index == market.quote_spot_market_index)
      .ok_or(anyhow::anyhow!("Spot market not found"))?;
    let spot_oracle = spot_market.decoded.oracle;
    let spot_oracle_source = spot_market.decoded.oracle_source;
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


  let res = arbiter.rpc().get_multiple_accounts_with_commitment(oracle_keys.as_slice(), CommitmentConfig::default()).await?;
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
      println!("perp oracle program: {}", raw.owner);
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

#[tokio::test]
async fn drift_spot_markets() -> anyhow::Result<()> {
  init_logger();
  dotenv::dotenv().ok();

  let arbiter = Arbiter::new_from_env().await?;

  struct MarketInfo {
    name: String,
    oracle: Pubkey,
    oracle_source: OracleSource,
    oracle_price_data: Option<OraclePriceData>,
    market_index: u16,
  }

  let spot_markets = DriftClient::spot_markets(arbiter.rpc()).await?;
  let mut oracles: Vec<MarketInfo> = vec![];
  for spot_market in spot_markets {
    let name = DriftClient::decode_name(&spot_market.decoded.name);
    let oracle = spot_market.decoded.oracle;
    let oracle_source = spot_market.decoded.oracle_source;

    oracles.push(MarketInfo {
      name,
      oracle,
      oracle_source,
      oracle_price_data: None,
      market_index: spot_market.decoded.market_index,
    });
  }

  let oracle_keys: Vec<Pubkey> = oracles.iter().map(|v| {
    v.oracle
  }).collect();

  let res = arbiter.rpc().get_multiple_accounts_with_commitment(oracle_keys.as_slice(), CommitmentConfig::default()).await?;
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
      let price_data = get_oracle_price(
        &market_info.oracle_source,
        &oracle_acct_info,
        slot
      ).map_err(|e| anyhow::anyhow!("Failed to get oracle price: {:?}", e))?;
      let price = price_data.price as f64 / PRICE_PRECISION as f64;

      println!("{} ({}): {}", market_info.name, market_info.market_index, price);
    }
  }

  Ok(())
}

/// cargo test --package arbiter --bin arbiter top_users -- --exact --show-output
#[tokio::test]
async fn top_users() -> anyhow::Result<()> {
  init_logger();
  dotenv::dotenv().ok();

  let arbiter = Arbiter::new_from_env().await?;

  let users = DriftClient::top_traders_by_pnl(&arbiter.nexus()).await?;
  println!("users: {}", users.len());
  let stats: Vec<nexus::TraderStub> = users.into_iter().map(|u| {
    nexus::TraderStub {
      authority: u.authority.to_string(),
      pnl: u.settled_perp_pnl(),
      best_user: u.best_user().key.to_string(),
    }
  }).collect();
  // write to json file
  let stats_json = serde_json::to_string(&stats)?;
  std::fs::write("traders.json", stats_json)?;
  Ok(())
}

/// cargo test --package arbiter --bin arbiter historical_pnl -- --exact --show-output
#[tokio::test]
async fn historical_pnl() -> anyhow::Result<()> {
  init_logger();
  dotenv::dotenv().ok();

  let arbiter = Arbiter::new_from_env().await?;

  let prefix = env!("CARGO_MANIFEST_DIR").to_string();

  let path = format!("{}/traders.json", prefix);
  let top_traders: Vec<nexus::TraderStub> = serde_json::from_str(&std::fs::read_to_string(path)?)?;
  // ordered least to greatest profit, so reverse order and take the best performers
  let top_traders: Vec<nexus::TraderStub> = top_traders.into_iter().rev().take(100).collect();
  let users: Vec<Pubkey> = top_traders.into_iter().flat_map(|t| {
    Pubkey::from_str(&t.best_user)
  }).collect();

  let mut top_dogs = vec![];
  for user in users {
    let data = DriftClient::drift_historical_pnl(
      &arbiter.nexus(),
      &user,
      100
    ).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    if data.dataset().len() > 1 && data.avg_quote_pnl() > 0.0 {
      top_dogs.push(data.clone());

      Plot::plot(
        vec![data.dataset()],
        &format!("{}/pnl/{}_cum_pnl.png", prefix, user),
        &format!("{} Performance", user),
        "Cum USDC PnL",
        "Unix Seconds",
      )?;
      log::info!("{} done", shorten_address(&user));
    }
  }

  let best: Vec<PnlStub> = top_dogs.into_iter().map(|d| {
    PnlStub {
      user: d.user(),
      avg_quote_pnl: d.avg_quote_pnl(),
    }
  }).collect();
  let json = serde_json::to_string(&best)?;
  std::fs::write("top_traders.json", json)?;

  Ok(())
}