use std::str::FromStr;

use crossbeam::channel::Sender;
use futures_util::future::try_join_all;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_rpc_client_api::config::RpcTransactionConfig;
use solana_rpc_client_api::response::RpcConfirmedTransactionStatusWithSignature;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use yellowstone_grpc_proto::prelude::subscribe_update::UpdateOneof;

use crate::{Cache, GrpcClient, Time, ToAccount};
use crate::types::*;

/// Registry of program account decoders that match a discriminant,
/// such as "User", to a specific account type.
#[derive(Clone)]
pub enum ProgramDecoder {
  Drift(drift_cpi::AccountType),
}

pub struct NexusClient {
  pub geyser: GrpcClient,
}

impl NexusClient {
  pub fn new(cfg: GeyserConfig) -> anyhow::Result<Self> {
    Ok(Self {
      geyser: GrpcClient::new(cfg),
    })
  }

  pub async fn stream(&self, cache: &RwLock<Cache>, channel: Sender<TxStub>) -> anyhow::Result<()> {
    let mut stream = self.geyser.subscribe().await?;
    while let Some(update) = stream.next().await {
      let update = update?;
      if let Some(update) = update.update_oneof {
        match update {
          UpdateOneof::Transaction(event) => {
            if let Some(tx_info) = event.transaction {
              if let Some(tx) = tx_info.transaction {
                if let Some(msg) = tx.message {
                  log::debug!("tx");
                  let account_keys: Vec<Pubkey> = msg.account_keys.iter().flat_map(|k| {
                    Pubkey::try_from(k.as_slice())
                  }).collect();
                  assert_eq!(account_keys.len(), msg.account_keys.len());

                  let mut ixs = vec![];
                  for ix in msg.instructions {
                    let program: Pubkey = *account_keys.get(ix.program_id_index as usize).ok_or(
                      anyhow::anyhow!("Program not found at account key index: {}", ix.program_id_index)
                    )?;
                    let accounts: Vec<Pubkey> = ix.accounts.iter().flat_map(|ix| {
                      account_keys.get(*ix as usize).cloned()
                    }).collect();
                    let data = ix.data.clone();

                    ixs.push(Ix {
                      program,
                      accounts,
                      data,
                    });
                  }

                  let signer = *account_keys.first().ok_or(
                    anyhow::anyhow!("Signer not found at account key index: 0")
                  )?;
                  let signature = Signature::try_from(
                    tx_info.signature.as_slice()
                  )?;
                  let hash_bytes: [u8; 32] = msg.recent_blockhash.try_into().map_err(|e| anyhow::anyhow!("Failed to convert blockhash: {:?}", e))?;
                  channel.send(TxStub {
                    slot: event.slot,
                    blockhash: Hash::from(hash_bytes).to_string(),
                    ixs,
                    signature,
                    signer,
                  })?;
                }
              }
            }
          }
          UpdateOneof::Account(event) => {
            if let Some(account) = event.account {
              let key = Pubkey::try_from(account.pubkey.as_slice()).map_err(
                |e| anyhow::anyhow!("Failed to convert pubkey: {:?}", e)
              )?;
              log::debug!("account: {}", &key);
              cache.write().await.ring_mut(key).insert(event.slot, AcctCtx {
                key,
                account: account.to_account()?,
                slot: event.slot,
              });
            }
          }
          UpdateOneof::BlockMeta(event) => {
            if let Some(block_time) = event.block_time {
              log::debug!("block meta: {}, {}", event.slot, event.blockhash);
              cache.write().await.blocks.insert(event.slot, BlockInfo {
                slot: event.slot,
                blockhash: event.blockhash,
                time: Time::from_unix(block_time.timestamp),
              });
            }
          }
          UpdateOneof::Slot(event) => {
            log::debug!("slot: {}", event.slot);
            if event.slot > cache.read().await.slot {
              cache.write().await.slot = event.slot;
            }
          }
          _ => {}
        }
      }
    }
    Ok(())
  }

  // ===================================================================================
  // HTTP API
  // ===================================================================================

  pub async fn historical_signatures(
    rpc: &RpcClient,
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
      let res = rpc.get_signatures_for_address_with_config(key, config).await?;
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

      let mut sigs: Vec<RpcConfirmedTransactionStatusWithSignature> = Vec::with_capacity(limit);

      // zeroth index is handled differently
      let zeroth = &chunks[0];
      let zeroth_cfg = GetConfirmedSignaturesForAddress2Config {
        limit: Some(zeroth.end - zeroth.start),
        ..Default::default()
      };
      let sigs_for_zeroth_chunk = rpc.get_signatures_for_address_with_config(key, zeroth_cfg).await?;
      let mut border_sig: RpcConfirmedTransactionStatusWithSignature = sigs_for_zeroth_chunk[sigs_for_zeroth_chunk.len() - 1].clone();
      sigs.extend(sigs_for_zeroth_chunk);

      // iterate everything after zeroth index
      let after_zeroth = &chunks[1..chunks.len() - 1];
      for chunk in after_zeroth {
        let cfg = GetConfirmedSignaturesForAddress2Config {
          limit: Some(chunk.end - chunk.start),
          before: Some(Signature::from_str(&border_sig.signature)?),
          ..Default::default()
        };
        let sigs_for_chunk = rpc.get_signatures_for_address_with_config(key, cfg).await?;
        border_sig = sigs_for_chunk[sigs_for_chunk.len() - 1].clone();
        sigs.extend(sigs_for_chunk);
      }

      Ok(sigs)
    }
  }

  pub async fn historical_transactions(
    rpc: &RpcClient,
    key: &Pubkey,
    limit: Option<usize>,
  ) -> anyhow::Result<Vec<TrxData>> {
    let sigs = Self::historical_signatures(
      rpc,
      key,
      limit,
    ).await?;

    let mut txs = Vec::<TrxData>::new();
    let opts = RpcTransactionConfig {
      max_supported_transaction_version: Some(1),
      ..Default::default()
    };
    for sig in sigs {
      let tx_info = rpc.get_transaction_with_config(&Signature::from_str(&sig.signature)?, opts).await?;

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
    rpc: &RpcClient,
    keys: &[Pubkey],
  ) -> anyhow::Result<Vec<AcctCtx>> {
    // get_multiple_accounts max Pubkeys is 100
    let chunk_size = 100;

    if keys.len() <= chunk_size {
      let pre_filter = rpc.get_multiple_accounts_with_commitment(keys, CommitmentConfig::confirmed()).await?;
      let accts = pre_filter.value;
      let slot = pre_filter.context.slot;
      let infos = accts.into_iter().enumerate().filter_map(|(index, acc)| {
        acc.map(|acc| AcctCtx {
          key: keys[index],
          account: acc,
          slot,
        })
      }).collect::<Vec<AcctCtx>>();
      Ok(infos)
    } else {
      let chunks = keys.chunks(chunk_size).collect::<Vec<&[Pubkey]>>();
      let infos = try_join_all(chunks.into_iter().enumerate().map(
        move |(_index, chunk)| async move {
          let res = rpc.get_multiple_accounts_with_commitment(chunk, CommitmentConfig::confirmed()).await?;
          let accs = res.value;
          let slot = res.context.slot;
          let accs = accs.into_iter().enumerate().flat_map(move |(index, acc)| {
            acc.map(|account| AcctCtx {
              key: chunk[index],
              account,
              slot,
            })
          });
          Result::<_, anyhow::Error>::Ok(accs)
        },
      )).await?.into_iter().flatten().collect::<Vec<AcctCtx>>();

      Ok(infos)
    }
  }
}