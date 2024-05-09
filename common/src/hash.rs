use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use crate::ArchiveAccount;

pub trait AccountTrait {
    fn key(&self) -> String;
    fn owner(&self) -> String;
    fn lamports(&self) -> u64;
    fn data(&self) -> &[u8];
}

#[derive(Debug, Default)]
pub struct AccountHasher(pub DefaultHasher);

pub trait HashTrait {
    fn new() -> Self;
    fn finish(&mut self) -> u64;
    fn hash_account<T: AccountTrait>(&mut self, account: &T) -> u64;
}

impl HashTrait for AccountHasher {
    fn new() -> Self {
        Self(DefaultHasher::new())
    }
    /// Reset contents of hasher for reuse
    fn finish(&mut self) -> u64 {
        self.0.finish()
    }
    /// Generate a hash for this account with this state
    fn hash_account<T: AccountTrait>(&mut self, account: &T) -> u64 {
        self.0 = DefaultHasher::new();
        account.key().hash(&mut self.0);
        account.owner().hash(&mut self.0);
        account.lamports().hash(&mut self.0);
        account.data().hash(&mut self.0);
        self.finish()
    }
}

impl AccountTrait for ArchiveAccount {
    fn key(&self) -> String {
        self.key.to_string()
    }
    fn owner(&self) -> String {
        self.owner.to_string()
    }
    fn lamports(&self) -> u64 {
        self.lamports
    }
    fn data(&self) -> &[u8] {
        self.data.as_slice()
    }
}
