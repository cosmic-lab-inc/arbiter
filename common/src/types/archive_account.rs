use crate::{AccountHasher, HashTrait};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ArchiveAccount {
    pub key: Pubkey,
    /// historical snapshot slot at which this state existed
    pub slot: u64,
    /// lamports in the account
    pub lamports: u64,
    /// the program that owns this account. If executable, the program that loads this account.
    pub owner: Pubkey,
    /// this account's data contains a loaded program (and is now read-only)
    pub executable: bool,
    /// the epoch at which this account will next owe rent
    pub rent_epoch: u64,
    /// data held in this account
    pub data: Vec<u8>,
}

impl ArchiveAccount {
    pub fn discrim(&self) -> Option<[u8; 8]> {
        if self.data.len() < 8 {
            return None;
        }
        let mut arr = [0u8; 8];
        arr.copy_from_slice(&self.data[..8]);
        Some(arr)
    }

    pub fn id(&self) -> u64 {
        let mut hasher = AccountHasher::new();
        hasher.hash_account(self)
    }
}
