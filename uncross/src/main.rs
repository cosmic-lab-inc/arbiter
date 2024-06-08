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
