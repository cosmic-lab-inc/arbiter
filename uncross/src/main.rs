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

use nexus::drift_client::*;
use nexus::drift_cpi::*;
use nexus::*;
use uncross::*;

mod config;
mod uncross;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  dotenv::dotenv().ok();
  init_logger();

  let mut client = Uncross::new(0, MarketId::SOL_PERP, None).await?;
  client.start().await?;

  Ok(())
}

// #[cfg(tests)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn arb_perp() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    init_logger();

    let client = Uncross::new(0, MarketId::SOL_PERP, None).await?;
    let mut trx = client.new_tx();

    let now = std::time::Instant::now();
    while now.elapsed() < std::time::Duration::from_secs(60 * 10) {
      let l3 = client
        .orderbook()
        .await
        .l3(&MarketId::SOL_PERP, &client.cache().await)?;
      let price = client
        .drift
        .market_info(MarketId::SOL_PERP, &client.cache().await, None)?
        .price;
      let slot = client.cache().await.slot;

      let bids_above = l3.uncross_bids(5.0, price, slot)?;
      let asks_below = l3.uncross_asks(5.0, price, slot)?;

      if !bids_above.is_empty() && !asks_below.is_empty() {
        let bid = bids_above.first().ok_or(anyhow::anyhow!("No bid"))?;
        let ask = asks_below.first().ok_or(anyhow::anyhow!("No ask"))?;
        if bid.price > ask.price {
          info!(
            "bid: ${}, ask: ${}",
            trunc!(bid.price, 3),
            trunc!(ask.price, 3)
          );

          let cache = client.cache().await;
          let bid_user = cache.decoded_account::<User>(&bid.user, None)?;
          let ask_user = cache.decoded_account::<User>(&ask.user, None)?;
          drop(cache);

          let bid_maker = MakerInfo {
            maker: bid.user,
            maker_user_stats: DriftUtils::user_stats_pda(&bid_user.decoded.authority),
            maker_user: bid_user.decoded,
          };
          let ask_maker = MakerInfo {
            maker: ask.user,
            maker_user_stats: DriftUtils::user_stats_pda(&ask_user.decoded.authority),
            maker_user: ask_user.decoded,
          };
          client
            .arb_perp(vec![bid_maker, ask_maker], &mut trx)
            .await?;
          info!("ixs");

          let res = trx.simulate(id()).await?.value;
          info!("{:#?}", res);
        }
      }

      tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    Ok(())
  }
}
