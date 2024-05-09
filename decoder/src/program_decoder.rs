use std::collections::HashMap;

use base64::{Engine as _, engine::general_purpose};
use borsh::{BorshDeserialize, BorshSerialize};
use log::{error, info};
use once_cell::sync::Lazy;
use serde_json::Value;
use sol_chainsaw::{ChainsawDeserializer, IdlProvider};
use solana_sdk::{hash::hash, pubkey::Pubkey};

use common::{DecodeProgramAccount, RegisteredType};

/// Master list of supported programs that can provide decoded accounts based on an Anchor IDL.
pub static PROGRAMS: Lazy<Vec<(String, Pubkey)>> =
  Lazy::new(|| vec![(drift_cpi::PROGRAM_NAME.clone(), *drift_cpi::PROGRAM_ID)]);

/// Registry of program account decoders that match a discriminant,
/// such as "User", to a specific account type.
#[derive(Copy, Clone, BorshDeserialize, BorshSerialize)]
pub enum Decoder {
  Drift(drift_cpi::AccountType),
}

pub struct ProgramDecoder {
  pub chainsaw: ChainsawDeserializer<'static>,
  pub idls: HashMap<Pubkey, String>,
}

impl ProgramDecoder {
  // TODO: get rid of chainsaw, all we need is the account discrim -> name lookup, which we can replicate.
  pub fn new() -> anyhow::Result<Self> {
    let mut chainsaw = ChainsawDeserializer::new(&*Box::leak(Box::default()));
    let mut idls = HashMap::new();

    for (_name, program) in PROGRAMS.iter() {
      let idl_path = format!("{}/idl.json", drift_cpi::PATH.clone());
      // let idl_path = format!("./idls/{}/idl.json", *name);
      info!("Load IDL at {}", idl_path);
      let idl = match std::fs::read_to_string(idl_path) {
        Ok(idl) => idl,
        Err(e) => {
          error!("Failed to read IDL path: {:?}", e);
          return Err(anyhow::Error::from(e));
        }
      };
      chainsaw.add_idl_json(program.to_string(), &idl, IdlProvider::Anchor)?;
      idls.insert(*program, idl);
    }

    Ok(Self { chainsaw, idls })
  }

  pub fn borsh_decode_account(
    &self,
    program_id: &Pubkey,
    account_name: &str,
    data: &[u8],
  ) -> anyhow::Result<Decoder> {
    match *program_id {
      _ if *program_id == *drift_cpi::PROGRAM_ID => Ok(Decoder::Drift(
        drift_cpi::AccountType::borsh_decode_account(account_name, data)?,
      )),
      _ => Err(anyhow::anyhow!(
                "Program {} not supported",
                program_id.to_string()
            )),
    }
  }

  pub fn json_decode_account(
    &self,
    program_id: &Pubkey,
    account_name: &str,
    data: &mut &[u8],
  ) -> anyhow::Result<Value> {
    match *program_id {
      _ if *program_id == *drift_cpi::PROGRAM_ID => {
        drift_cpi::AccountType::json_decode_account(
          &self.chainsaw,
          program_id,
          account_name,
          data,
        )
      }
      _ => Err(anyhow::anyhow!(
                "Program {} not supported",
                program_id.to_string()
            )),
    }
  }

  fn idl(&self, program_id: &Pubkey) -> anyhow::Result<String> {
    self.idls
        .get(program_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("No IDL found for program"))
  }

  pub fn registered_types(&self) -> anyhow::Result<Vec<RegisteredType>> {
    let registered_types: Vec<anyhow::Result<Vec<RegisteredType>>> = PROGRAMS
      .iter()
      .map(|(_, program_id)| self.program_registered_types(program_id))
      .collect();
    Ok(registered_types
      .into_iter()
      .filter_map(Result::ok)
      .flatten()
      .collect())
  }

  pub fn program_registered_types(
    &self,
    program_id: &Pubkey,
  ) -> anyhow::Result<Vec<RegisteredType>> {
    let idl_str = self.idl(program_id)?;
    let idl = serde_json::from_str::<Value>(&idl_str)?;
    let accounts = serde_json::from_value::<Vec<Value>>(idl["accounts"].clone())?;
    let types = serde_json::from_value::<Vec<Value>>(idl["types"].clone())?;
    let schemas = accounts
      .into_iter()
      .chain(types)
      .map(|raw_acct| RegisteredType {
        program_name: idl["name"].as_str().unwrap().to_string(),
        program: *program_id,
        discriminant: raw_acct["name"].as_str().unwrap().to_string(),
        schema: raw_acct,
      })
      .collect::<Vec<RegisteredType>>();
    Ok(schemas)
  }

  pub fn name_to_discrim(&self, account_name: &str) -> [u8; 8] {
    Self::account_discriminator(account_name)
  }

  pub fn discrim_to_name(
    &self,
    program_id: &Pubkey,
    account_discrim: &[u8; 8],
  ) -> Option<String> {
    self.chainsaw
        .account_name(&program_id.to_string(), account_discrim)
        .map(|name| name.to_string())
  }

  pub fn name_to_base64_discrim(account_name: &str) -> String {
    let bytes = Self::account_discriminator(account_name);
    general_purpose::STANDARD.encode(bytes)
  }

  pub fn base64_discrim_to_name(
    &self,
    program_id: &Pubkey,
    base64_discrim: &str,
  ) -> anyhow::Result<String> {
    let bytes = general_purpose::STANDARD.decode(base64_discrim)?;
    let discrim: [u8; 8] = bytes[..8].try_into()?;
    match self.discrim_to_name(program_id, &discrim) {
      Some(name) => Ok(name),
      None => Err(anyhow::anyhow!("No name found for base64 discriminator")),
    }
  }

  /// Derives the account discriminator form the account name as Anchor does.
  pub fn account_discriminator(name: &str) -> [u8; 8] {
    let mut discriminator = [0u8; 8];
    let hashed = hash(format!("account:{}", name).as_bytes()).to_bytes();
    discriminator.copy_from_slice(&hashed[..8]);
    discriminator
  }
}
