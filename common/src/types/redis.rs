use crate::{deserialize_pubkey, serialize_pubkey};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RedisUser {
    pub api_key: String,
    #[serde(
        serialize_with = "serialize_pubkey",
        deserialize_with = "deserialize_pubkey"
    )]
    pub profile: Pubkey,
}
