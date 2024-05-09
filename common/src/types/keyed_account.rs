use solana_sdk::pubkey::Pubkey;

pub struct KeyedAccount<T> {
    pub key: Pubkey,
    pub account: T,
}
