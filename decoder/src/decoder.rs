use std::collections::HashMap;
use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose;
use borsh::{BorshDeserialize, BorshSerialize};
use futures::future::try_join_all;
use log::{error, info};
use once_cell::sync::Lazy;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_client::rpc_response::RpcConfirmedTransactionStatusWithSignature;
use solana_sdk::account::Account;
use solana_sdk::hash::hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use heck::ToSnakeCase;

use common::{Chunk, KeyedAccount, TrxData};
use drift_cpi::DecodeAccount;

/// Master list of supported programs that can provide decoded accounts based on an Anchor IDL.
pub static PROGRAMS: Lazy<Vec<(String, Pubkey)>> =
  Lazy::new(|| vec![(drift_cpi::PROGRAM_NAME.clone(), *drift_cpi::PROGRAM_ID)]);

/// Registry of program account decoders that match a discriminant,
/// such as "User", to a specific account type.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum ProgramDecoder {
  Drift(drift_cpi::AccountType),
}

pub struct Decoder {
  pub idls: HashMap<Pubkey, String>,
  pub program_account_names: HashMap<Pubkey, Vec<String>>,
  pub program_instruction_names: HashMap<Pubkey, Vec<String>>,
}

impl Decoder {
  pub fn new() -> anyhow::Result<Self> {
    let mut idls = HashMap::new();
    let mut program_account_names = HashMap::new();
    let mut program_instruction_names = HashMap::new();

    for (_name, program) in PROGRAMS.iter() {
      let idl_path = format!("{}/idl.json", drift_cpi::PATH.clone());
      // let idl_path = format!("./idls/{}/idl.json", *name);
      info!("Load IDL at {}", idl_path);
      let idl_str = match std::fs::read_to_string(idl_path) {
        Ok(idl) => idl,
        Err(e) => {
          error!("Failed to read IDL path: {:?}", e);
          return Err(anyhow::Error::from(e));
        }
      };

      let idl = serde_json::from_str::<serde_json::Value>(&idl_str).unwrap();
      let accounts = serde_json::from_value::<Vec<serde_json::Value>>(idl["accounts"].clone()).unwrap();
      let account_names = accounts.iter().map(|account| account["name"].as_str().unwrap().to_string()).collect::<Vec<String>>();
      let ixs = serde_json::from_value::<Vec<serde_json::Value>>(idl["instructions"].clone()).unwrap();
      let ix_names = ixs.iter().map(|ix| ix["name"].as_str().unwrap().to_string()).collect::<Vec<String>>();

      idls.insert(*program, idl_str);
      program_account_names.insert(*program, account_names);
      program_instruction_names.insert(*program, ix_names);
    }

    Ok(Self { idls, program_account_names, program_instruction_names })
  }

  pub fn de(
    program_id: &Pubkey,
    account_name: &str,
    data: &[u8],
  ) -> anyhow::Result<ProgramDecoder> {
    match *program_id {
      _ if *program_id == *drift_cpi::PROGRAM_ID => Ok(ProgramDecoder::Drift(
        drift_cpi::AccountType::decode(account_name, data).map_err(
          |e| anyhow::anyhow!("Failed to decode account: {:?}", e)
        )?
      )),
      _ => Err(anyhow::anyhow!(
          "Program {} not supported",
          program_id.to_string()
      )),
    }
  }

  /// Derives the account discriminator form the account name as Anchor does.
  pub fn account_discriminator(name: &str) -> [u8; 8] {
    let mut discriminator = [0u8; 8];
    let hashed = hash(format!("account:{}", name).as_bytes()).to_bytes();
    discriminator.copy_from_slice(&hashed[..8]);
    discriminator
  }

  pub fn instruction_discriminator(name: &str) -> [u8; 8] {
    let name = name.to_snake_case();
    let mut discriminator = [0u8; 8];
    let hashed = hash(format!("global:{}", name).as_bytes()).to_bytes();
    discriminator.copy_from_slice(&hashed[..8]);
    discriminator
  }

  pub fn account_discrim_to_name(
    &self,
    program_id: &Pubkey,
    account_discrim: &[u8; 8],
  ) -> anyhow::Result<Option<String>> {
    let names = self.program_account_names.get(program_id).ok_or(anyhow::anyhow!("Program not found"))?;
    let name = names.iter().find(|name| {
      let bytes = Self::account_discriminator(name);
      bytes == *account_discrim
    }).cloned();
    Ok(name)
  }

  pub fn instruction_discrim_to_name(
    &self,
    program_id: &Pubkey,
    ix_discrim: &[u8; 8],
  ) -> anyhow::Result<Option<String>> {
    let names = self.program_instruction_names.get(program_id).ok_or(anyhow::anyhow!("Program not found"))?;
    let name = names.iter().find(|name| {
      let bytes = Self::instruction_discriminator(name);
      bytes == *ix_discrim
    }).cloned();
    Ok(name)
  }

  pub fn account_name_to_base64_discrim(account_name: &str) -> String {
    let bytes = Self::account_discriminator(account_name);
    general_purpose::STANDARD.encode(bytes)
  }

  pub fn instruction_name_to_base64_discrim(account_name: &str) -> String {
    let bytes = Self::instruction_discriminator(account_name);
    general_purpose::STANDARD.encode(bytes)
  }

  pub fn account_base64_discrim_to_name(
    &self,
    program_id: &Pubkey,
    base64_discrim: &str,
  ) -> anyhow::Result<String> {
    let bytes = general_purpose::STANDARD.decode(base64_discrim)?;
    let discrim: [u8; 8] = bytes[..8].try_into()?;
    match self.account_discrim_to_name(program_id, &discrim)? {
      Some(name) => Ok(name),
      None => Err(anyhow::anyhow!("No name found for base64 discriminator")),
    }
  }

  pub fn instruction_base64_discrim_to_name(
    &self,
    program_id: &Pubkey,
    base64_discrim: &str,
  ) -> anyhow::Result<String> {
    let bytes = general_purpose::STANDARD.decode(base64_discrim)?;
    let discrim: [u8; 8] = bytes[..8].try_into()?;
    match self.instruction_discrim_to_name(program_id, &discrim)? {
      Some(name) => Ok(name),
      None => Err(anyhow::anyhow!("No name found for base64 discriminator")),
    }
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
      max_supported_transaction_version: Some(1),
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
}