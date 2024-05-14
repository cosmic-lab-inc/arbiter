use solana_account_decoder::{UiAccount, UiAccountEncoding};
use solana_rpc_client_api::config::RpcAccountInfoConfig;
use solana_rpc_client_api::response::Response;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use crate::types::*;
use crate::websocket::*;
use std::str::FromStr;
use std::time::Duration;
use futures_util::future::try_join_all;
use once_cell::sync::Lazy;
use reqwest::Client;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_client::rpc_response::RpcConfirmedTransactionStatusWithSignature;
use solana_sdk::account::Account;
use solana_sdk::signature::Signature;
use common::{Chunk, KeyedAccount, TrxData};
use crate::drift_cpi::Decode;

/// Master list of supported programs that can provide decoded accounts based on an Anchor IDL.
pub static PROGRAMS: Lazy<Vec<(String, Pubkey)>> =
  Lazy::new(|| vec![(drift_cpi::PROGRAM_NAME.clone(), drift_cpi::id())]);

/// Registry of program account decoders that match a discriminant,
/// such as "User", to a specific account type.
#[derive(Clone)]
pub enum ProgramDecoder {
  Drift(drift_cpi::AccountType),
}

pub struct Nexus {
  pub rpc: RpcClient,
  pub ws: NexusClient,
  pub client: Client,
}

impl Nexus {
  pub async fn new(rpc: &str, wss: &str) -> anyhow::Result<Self> {
    Ok(Self {
      rpc: RpcClient::new_with_timeout_and_commitment(
        rpc.to_string(),
        Duration::from_secs(90),
        CommitmentConfig::confirmed(),
      ),
      ws: NexusClient::new(wss).await?,
      client: Client::builder().timeout(Duration::from_secs(90)).build()?,
    })
  }

  /// Assumes .env contains key "RPC_URL" with HTTP endpoint.
  /// Assumes .env contains key "WS_URL" with WSS endpoint.
  pub async fn new_from_env() -> anyhow::Result<Self> {
    let rpc = std::env::var("RPC_URL")?;
    let wss = std::env::var("WS_URL")?;
    Self::new(&rpc, &wss).await
  }

  // ===================================================================================
  // Deserialization
  // ===================================================================================

  pub fn decode_program_account(
    program_id: &Pubkey,
    data: &[u8],
  ) -> anyhow::Result<ProgramDecoder> {
    match *program_id {
      _ if *program_id == drift_cpi::id() => Ok(ProgramDecoder::Drift(
        drift_cpi::AccountType::decode(data).map_err(
          |e| anyhow::anyhow!("Failed to decode account: {:?}", e)
        )?
      )),
      _ => Err(anyhow::anyhow!(
          "Program {} not supported",
          program_id.to_string()
      )),
    }
  }

  // ===================================================================================
  // HTTP API
  // ===================================================================================

  pub async fn historical_signatures(
    &self,
    key: &Pubkey,
    limit: Option<usize>,
  ) -> anyhow::Result<Vec<RpcConfirmedTransactionStatusWithSignature>> {
    let limit = limit.unwrap_or(1000);

    if limit <= 1000 {
      let config = GetConfirmedSignaturesForAddress2Config {
        limit: Some(limit),
        ..Default::default()
      };
      // by default this fetches last 1000 signatures
      let res = self.rpc
                    .get_signatures_for_address_with_config(key, config)
                    .await?;
      Ok(res)
    } else {
      let mut chunks: Vec<Chunk> = Vec::new();
      let mut eat_limit = limit;
      let chunk_size = 1000;
      while eat_limit > 0 {
        let start = limit - eat_limit;
        let end = std::cmp::min(start + chunk_size, limit);
        eat_limit -= &chunk_size;
        chunks.push(Chunk { start, end });
      }

      let mut sigs: Vec<RpcConfirmedTransactionStatusWithSignature> =
        Vec::with_capacity(limit);

      // zeroth index is handled differently
      let zeroth = &chunks[0];
      let zeroth_cfg = GetConfirmedSignaturesForAddress2Config {
        limit: Some(zeroth.end - zeroth.start),
        ..Default::default()
      };
      let sigs_for_zeroth_chunk = self.rpc
                                      .get_signatures_for_address_with_config(key, zeroth_cfg)
                                      .await?;
      let mut border_sig: RpcConfirmedTransactionStatusWithSignature =
        sigs_for_zeroth_chunk[sigs_for_zeroth_chunk.len() - 1].clone();
      sigs.extend(sigs_for_zeroth_chunk);

      // iterate everything after zeroth index
      let after_zeroth = &chunks[1..chunks.len() - 1];
      for chunk in after_zeroth {
        let cfg = GetConfirmedSignaturesForAddress2Config {
          limit: Some(chunk.end - chunk.start),
          before: Some(Signature::from_str(&border_sig.signature)?),
          ..Default::default()
        };
        let sigs_for_chunk = self.rpc
                                 .get_signatures_for_address_with_config(key, cfg)
                                 .await?;
        border_sig = sigs_for_chunk[sigs_for_chunk.len() - 1].clone();
        sigs.extend(sigs_for_chunk);
      }

      Ok(sigs)
    }
  }

  pub async fn historical_transactions(
    &self,
    key: &Pubkey,
    limit: Option<usize>,
  ) -> anyhow::Result<Vec<TrxData>> {
    let sigs = self.historical_signatures(
      key,
      limit
    ).await?;

    let mut txs = Vec::<TrxData>::new();
    let opts = RpcTransactionConfig {
      max_supported_transaction_version: Some(1),
      ..Default::default()
    };
    for sig in sigs {
      let tx_info = self.rpc
                        .get_transaction_with_config(&Signature::from_str(&sig.signature)?, opts)
                        .await?;

      let decoded_tx = tx_info.transaction.transaction.decode();
      if let Some(decoded_tx) = decoded_tx {
        let signature = Signature::from_str(&sig.signature)?;
        let tx = decoded_tx.into_legacy_transaction();
        if let Some(tx) = &tx {
          let signer = tx.message.account_keys.first();
          if let Some(signer) = signer {
            let trx_data = TrxData {
              tx: tx.clone(),
              signature,
              signer: *signer,
              slot: tx_info.slot,
              block_time: tx_info.block_time.unwrap_or(0),
            };
            txs.push(trx_data);
          }
        }
      }
    }
    Ok(txs)
  }

  pub async fn accounts(
    &self,
    keys: &[Pubkey],
  ) -> anyhow::Result<Vec<KeyedAccount<Account>>> {
    // get_multiple_accounts max Pubkeys is 100
    let chunk_size = 100;

    if keys.len() <= chunk_size {
      let pre_filter = self.rpc.get_multiple_accounts(keys).await?;
      let infos = pre_filter
        .into_iter()
        .enumerate()
        .filter_map(|(index, acc)| {
          acc.map(|acc| KeyedAccount {
            key: keys[index],
            account: acc,
          })
        })
        .collect::<Vec<KeyedAccount<Account>>>();
      Ok(infos)
    } else {
      let chunks = keys.chunks(chunk_size).collect::<Vec<&[Pubkey]>>();
      let infos = try_join_all(chunks.into_iter().enumerate().map(
        move |(_index, chunk)| async move {
          let accs = self.rpc
                         .get_multiple_accounts(chunk)
                         .await?
            .into_iter()
            .enumerate()
            .filter_map(|(index, acc)| {
              acc.map(|acc| KeyedAccount {
                key: chunk[index],
                account: acc,
              })
            });
          Result::<_, anyhow::Error>::Ok(accs)
        },
      ))
        .await?
        .into_iter()
        .flatten()
        .collect::<Vec<KeyedAccount<Account>>>();

      Ok(infos)
    }
  }

  // ===================================================================================
  // Geyser WS API
  // ===================================================================================

  pub async fn stream_transactions(&self, key: &Pubkey) -> anyhow::Result<(EventStream<TransactionNotification>, StreamUnsub)> {
    let config = RpcTransactionsConfig {
      filter: TransactionSubscribeFilter::standard(key),
      options: TransactionSubscribeOptions::default()
    };
    let (stream, unsub) = self.ws.transaction_subscribe(config).await?;
    Ok((stream, unsub))
  }

  pub async fn stream_accounts(&self, key: &Pubkey) -> anyhow::Result<(EventStream<Response<UiAccount>>, StreamUnsub)> {
    let config = RpcAccountInfoConfig {
      encoding: Some(UiAccountEncoding::Base64),
      commitment: Some(CommitmentConfig::confirmed()),
      ..Default::default()
    };
    let (stream, unsub) = self.ws.account_subscribe(key, Some(config)).await?;
    Ok((stream, unsub))
  }
}