#![allow(unused_imports)]

use std::time::Duration;
use borsh::BorshDeserialize;
use reqwest::Client;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use tokio::io::ReadBuf;

use common::*;
use decoder::{HistoricalPerformance, HistoricalSettlePnl};
use crate::Time;

pub struct Arbiter {
  pub signer: Keypair,
  pub rpc: RpcClient,
  pub client: Client
}

impl Arbiter {
  pub async fn new(signer: Keypair, rpc_url: String) -> anyhow::Result<Self> {
    Ok(Self {
      signer,
      rpc: RpcClient::new_with_timeout_and_commitment(
        rpc_url,
        Duration::from_secs(90),
        CommitmentConfig::confirmed(),
      ),
      client: Client::builder().timeout(Duration::from_secs(90)).build()?
    })
  }

  pub fn rpc(&self) -> &RpcClient {
    &self.rpc
  }

  pub fn read_keypair_from_env(env_key: &str) -> anyhow::Result<Keypair> {
    read_keypair_from_env(env_key)
  }

  pub async fn drift_historical_pnl(
    &self,
    user: &Pubkey,
    days_back: i64
  ) -> anyhow::Result<HistoricalPerformance> {
    let end = Time::now();
    // drift doesn't have anything more recent than 2 days ago
    let end = end.delta_date(-2);

    let mut data = vec![];
    for i in 0..days_back {
      let date = end.delta_date(-i);

      let url = format!(
        "{}user/{}/settlePnlRecords/{}/{}{}{}",
        DRIFT_API_PREFIX,
        user,
        date.year,
        date.year,
        date.month.to_mm(),
        date.day.to_dd()
      );

      let res = self
        .client
        .get(url.clone())
        // gzip header
        .header("Accept-Encoding", "gzip")
        .send()
        .await?;
      if res.status().is_success() {
        let bytes = res.bytes().await?;
        let decoder = flate2::read::GzDecoder::new(bytes.as_ref());
        let mut rdr = csv::ReaderBuilder::new().from_reader(decoder);

        for result in rdr.records() {
          let record = result?;
          let datum = record.deserialize::<HistoricalSettlePnl>(None)?;
          data.push(datum);
        }
      } else if res.status() != 403 {
        log::error!(
          "Failed to get historical Drift data with status: {}, for user {} and date: {}/{}/{}", 
          res.status(),
          user,
          date.year,
          date.month.to_mm(),
          date.day.to_dd()
        );
      }
    }
    // sort data so latest `ts` field (timestamp) is last index
    data.sort_by_key(|a| a.ts);

    Ok(HistoricalPerformance(data))
  }
}

pub const DRIFT_API_PREFIX: &str = "https://drift-historical-data-v2.s3.eu-west-1.amazonaws.com/program/dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH/";
