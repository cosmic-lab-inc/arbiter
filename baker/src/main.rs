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

use baker::*;
use nexus::drift_client::*;
use nexus::drift_cpi::*;
use nexus::*;

mod baker;
mod config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  dotenv::dotenv().ok();
  init_logger();

  // SOL-PERP
  let market = MarketId::perp(0);
  let mut baker = Baker::new(0, market, None).await?;
  baker.start().await?;

  Ok(())
}

#[cfg(tests)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn close_positions() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    init_logger();

    let market = MarketId::perp(0);
    let baker = Baker::new(0, market, None).await?;
    let mut trx = baker.new_tx();
    trx = trx.retry_until_confirmed();
    baker
      .close_perp_positions(vec![market].as_slice(), &mut trx)
      .await?;
    trx.send_tx(id(), None).await?;

    Ok(())
  }
}
