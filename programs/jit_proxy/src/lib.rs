#![allow(clippy::too_many_arguments)]

pub use anchor_gen::prelude::*;

generate_cpi_crate!("idl.json");
declare_id!("J1TnP8zvVxbtF5KFp5xRmWuvG9McnhzmBd9XGfCyuxFP");

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct ArbPerpParams {
  pub market_index: u16,
}

impl anchor_lang::Discriminator for ArbPerpParams {
  const DISCRIMINATOR: [u8; 8] = [116, 105, 138, 99, 28, 171, 39, 225];
}
impl anchor_lang::InstructionData for ArbPerpParams {}
impl anchor_lang::Owner for ArbPerpParams {
  fn owner() -> Pubkey {
    ID
  }
}

pub struct ArbPerpCtx {
  pub state: Pubkey,
  pub user: Pubkey,
  pub user_stats: Pubkey,
  pub authority: Pubkey,
  pub drift_program: Pubkey,
}

impl anchor_lang::ToAccountMetas for ArbPerpCtx {
  fn to_account_metas(&self, _: Option<bool>) -> Vec<AccountMeta> {
    let state = AccountMeta {
      pubkey: self.state,
      is_writable: false,
      is_signer: false,
    };
    let user = AccountMeta {
      pubkey: self.user,
      is_writable: true,
      is_signer: false,
    };
    let user_stats = AccountMeta {
      pubkey: self.user_stats,
      is_writable: true,
      is_signer: false,
    };
    let authority = AccountMeta {
      pubkey: self.authority,
      is_writable: false,
      is_signer: true,
    };
    let drift_program = AccountMeta {
      pubkey: self.drift_program,
      is_writable: false,
      is_signer: false,
    };
    vec![state, user, user_stats, authority, drift_program]
  }
}

#[test]
pub fn instruction_discriminator() {
  let name = "arb_perp";
  let mut discriminator = [0u8; 8];
  let hashed = solana_sdk::hash::hash(format!("global:{}", name).as_bytes()).to_bytes();
  discriminator.copy_from_slice(&hashed[..8]);
  println!("{:?}", discriminator);
}
