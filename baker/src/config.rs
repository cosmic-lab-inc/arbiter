use std::{path::PathBuf, str::FromStr};

use chrono::{DateTime, Utc};
use nexus::read_keypair_from_env;
use serde::{Deserialize, Deserializer};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;

#[derive(Debug, Deserialize)]
pub struct BakerConfig {
  pub read_only: bool,
  pub retry_until_confirmed: bool,
  #[serde(deserialize_with = "BakerConfig::deserialize_keypair")]
  pub signer: Keypair,
  pub rpc_url: String,
  pub grpc: String,
  pub x_token: String,
  pub pct_cancel_threshold: f64,
}

#[derive(Debug, Deserialize)]
struct YamlConfig {
  pub read_only: bool,
  pub retry_until_confirmed: bool,
  pub grpc: String,
  pub pct_cancel_threshold: f64,
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
      pct_cancel_threshold: yaml.pct_cancel_threshold,
    })
  }
}
