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
use switzerland::*;

mod config;
mod data;
mod switzerland;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  dotenv::dotenv().ok();
  init_logger();

  // SOL-PERP
  let market = MarketId::SOL_PERP;
  let mut client = Switzerland::new(0, market).await?;
  client.start().await?;

  Ok(())
}
