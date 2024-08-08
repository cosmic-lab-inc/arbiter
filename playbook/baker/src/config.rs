use std::sync::Arc;
use std::{path::PathBuf, str::FromStr};

use chrono::{DateTime, Utc};
use nexus::read_keypair_from_env;
use serde::{Deserialize, Deserializer};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;

#[derive(Debug)]
pub struct BakerConfig {
  pub read_only: bool,
  pub retry_until_confirmed: bool,
  pub signer: Keypair,
  pub rpc_url: String,
  pub grpc: String,
  pub x_token: String,
  pub pct_spread_multiplier: f64,
  pub pct_stop_loss: f64,
  pub leverage: f64,
  pub pct_max_spread: f64,
  pub pct_min_spread: f64,
  pub stop_loss_is_maker: bool,
  pub attempt_breakeven: bool,
}

#[derive(Debug, Deserialize)]
struct YamlConfig {
  pub read_only: bool,
  pub retry_until_confirmed: bool,
  pub grpc: String,
  pub pct_spread_multiplier: f64,
  pub pct_stop_loss: f64,
  pub leverage: f64,
  pub pct_max_spread: f64,
  pub pct_min_spread: f64,
  pub stop_loss_is_maker: bool,
  pub attempt_breakeven: bool,
}

impl BakerConfig {
  fn deserialize_keypair<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Keypair, D::Error> {
    let kp_bytes: Vec<u8> = match Vec::deserialize(deserializer) {
      Ok(res) => res,
      Err(e) => {
        return Err(serde::de::Error::custom(format!(
          "Failed to deserialize keypair bytes: {}",
          e
        )))
      }
    };
    Keypair::from_bytes(&kp_bytes)
      .map_err(|e| serde::de::Error::custom(format!("Failed to deserialize keypair bytes: {}", e)))
  }

  pub fn read() -> anyhow::Result<Self> {
    let dir = env!("CARGO_MANIFEST_DIR").to_string();
    let name = "config.yaml";
    let path = format!("{}/{}", dir, name);
    let path = PathBuf::from_str(&path)?;
    let contents = String::from_utf8(std::fs::read(path)?)?;
    let yaml: YamlConfig = serde_yaml::from_str(&contents)?;
    let x_token = std::env::var("X_TOKEN")?;
    let signer = read_keypair_from_env("SIGNER")?;
    let rpc_url = std::env::var("RPC_URL")?;
    Ok(Self {
      signer,
      x_token,
      rpc_url,
      read_only: yaml.read_only,
      retry_until_confirmed: yaml.retry_until_confirmed,
      grpc: yaml.grpc,
      pct_spread_multiplier: yaml.pct_spread_multiplier,
      pct_stop_loss: yaml.pct_stop_loss,
      leverage: yaml.leverage,
      pct_max_spread: yaml.pct_max_spread,
      pct_min_spread: yaml.pct_min_spread,
      stop_loss_is_maker: yaml.stop_loss_is_maker,
      attempt_breakeven: yaml.attempt_breakeven,
    })
  }
}
