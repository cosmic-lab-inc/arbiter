use crate::ArchiveAccount;
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, BorshDeserialize, BorshSerialize)]
pub struct EpochAccount {
    /// hash that is unique to the key at this slot
    pub id: u64,
    /// address of this account on-chain
    pub key: String,
    /// historical snapshot slot at which this state existed
    pub slot: u64,
    /// lamports in the account
    pub lamports: u64,
    /// the program that owns this account. If executable, the program that loads this account.
    pub owner: String,
    /// this account's data contains a loaded program (and is now read-only)
    pub executable: bool,
    /// the epoch at which this account will next owe rent
    pub rent_epoch: u64,
    /// first 8 bytes of the data that Anchor uses to determine the program account type.
    pub discriminant: Option<[u8; 8]>,
    /// data held in this account
    pub data: Vec<u8>,
}

impl TryFrom<ArchiveAccount> for EpochAccount {
    type Error = anyhow::Error;
    fn try_from(account: ArchiveAccount) -> anyhow::Result<Self> {
        Ok(Self {
            id: account.id(),
            key: account.key.to_string(),
            slot: account.slot,
            lamports: account.lamports,
            owner: account.owner.to_string(),
            executable: account.executable,
            discriminant: account.discrim(),
            rent_epoch: account.rent_epoch,
            data: account.data,
        })
    }
}
