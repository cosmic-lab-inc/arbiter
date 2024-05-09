use crate::{
    deserialize_option_pubkey, deserialize_pubkey, serialize_option_pubkey, serialize_pubkey,
};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryAccountId {
    pub id: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct QueryAccounts {
    #[serde(deserialize_with = "deserialize_option_pubkey")]
    #[serde(serialize_with = "serialize_option_pubkey")]
    #[serde(default)]
    pub key: Option<Pubkey>,
    #[serde(default)]
    pub slot: Option<u64>,
    #[serde(default)]
    pub min_slot: Option<u64>,
    #[serde(default)]
    pub max_slot: Option<u64>,
    #[serde(deserialize_with = "deserialize_option_pubkey")]
    #[serde(serialize_with = "serialize_option_pubkey")]
    #[serde(default)]
    pub owner: Option<Pubkey>,
    #[serde(default)]
    pub limit: Option<u64>,
    #[serde(default)]
    pub offset: Option<u64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct QueryDecodedAccounts {
    #[serde(deserialize_with = "deserialize_option_pubkey")]
    #[serde(serialize_with = "serialize_option_pubkey")]
    #[serde(default)]
    pub key: Option<Pubkey>,
    #[serde(default)]
    pub slot: Option<u64>,
    #[serde(default)]
    pub min_slot: Option<u64>,
    #[serde(default)]
    pub max_slot: Option<u64>,
    #[serde(serialize_with = "serialize_pubkey")]
    #[serde(deserialize_with = "deserialize_pubkey")]
    pub owner: Pubkey,
    pub discriminant: String,
    #[serde(default)]
    pub limit: Option<u64>,
    #[serde(default)]
    pub offset: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryRegisteredTypes {
    #[serde(default)]
    pub program_name: Option<String>,
    #[serde(deserialize_with = "deserialize_option_pubkey")]
    #[serde(serialize_with = "serialize_option_pubkey")]
    #[serde(default)]
    pub program: Option<Pubkey>,
    #[serde(default)]
    pub discriminant: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisteredType {
    pub program_name: String,
    #[serde(serialize_with = "serialize_pubkey")]
    #[serde(deserialize_with = "deserialize_pubkey")]
    pub program: Pubkey,
    pub discriminant: String,
    pub schema: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EpochProfile {
    #[serde(serialize_with = "serialize_pubkey")]
    #[serde(deserialize_with = "deserialize_pubkey")]
    pub profile: Pubkey,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestAirdrop {
    #[serde(serialize_with = "serialize_pubkey")]
    #[serde(deserialize_with = "deserialize_pubkey")]
    pub key: Pubkey,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestChallenge {
    #[serde(serialize_with = "serialize_pubkey")]
    #[serde(deserialize_with = "deserialize_pubkey")]
    pub key: Pubkey,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthenticateSignature {
    #[serde(serialize_with = "serialize_pubkey")]
    #[serde(deserialize_with = "deserialize_pubkey")]
    pub key: Pubkey,
    /// Base58 encoded signature
    pub signature: String,
}
