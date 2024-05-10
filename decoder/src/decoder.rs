use std::fmt::Debug;
use std::mem::size_of;
use std::str::FromStr;

use bytemuck::checked::try_from_bytes;
use bytemuck::CheckedBitPattern;
use futures::future::try_join_all;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_client::rpc_response::RpcConfirmedTransactionStatusWithSignature;
use solana_sdk::account::Account;
use solana_sdk::hash::hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;

use common::{Chunk, KeyedAccount, TrxData};

pub struct Decoder;

impl Decoder {
  pub fn de<T: CheckedBitPattern + Debug>(account_buffer: &[u8]) -> anyhow::Result<&T> {
    try_from_bytes(&account_buffer[8..][..size_of::<T>()]).map_err(Into::into)
  }

  /// Derives the account discriminator form the account name as Anchor does.
  pub fn account_discriminator(name: &str) -> [u8; 8] {
    let mut discriminator = [0u8; 8];
    let hashed = hash(format!("account:{}", name).as_bytes()).to_bytes();
    discriminator.copy_from_slice(&hashed[..8]);
    discriminator
  }

  pub async fn accounts(
    client: &RpcClient,
    keys: &[Pubkey],
  ) -> anyhow::Result<Vec<KeyedAccount<Account>>> {
    // get_multiple_accounts max Pubkeys is 100
    let chunk_size = 100;

    if keys.len() <= chunk_size {
      let pre_filter = client.get_multiple_accounts(keys).await?;
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
          let accs = client
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

  pub async fn signatures(
    client: &RpcClient,
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
      let res = client
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
      let sigs_for_zeroth_chunk = client
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
        let sigs_for_chunk = client
          .get_signatures_for_address_with_config(key, cfg)
          .await?;
        border_sig = sigs_for_chunk[sigs_for_chunk.len() - 1].clone();
        sigs.extend(sigs_for_chunk);
      }

      Ok(sigs)
    }
  }

  pub async fn transactions(
    client: &RpcClient,
    key: &Pubkey,
    limit: Option<usize>,
  ) -> anyhow::Result<Vec<TrxData>> {
    let sigs = Self::signatures(
      client,
      key,
      limit
    ).await?;

    let mut txs = Vec::<TrxData>::new();
    let opts = RpcTransactionConfig {
      max_supported_transaction_version: Some(0),
      ..Default::default()
    };
    for sig in sigs {
      let tx_info = client
        .get_transaction_with_config(&Signature::from_str(&sig.signature)?, opts)
        .await?;

      let decoded_tx = tx_info.transaction.transaction.decode();
      if let Some(decoded_tx) = decoded_tx {
        let signature = Signature::from_str(&sig.signature)?;
        let tx = decoded_tx.into_legacy_transaction();
        if let Some(tx) = &tx {
          let signer = tx.message.account_keys.get(0);
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
}