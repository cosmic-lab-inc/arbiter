use crate::Decoder;
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

#[derive(Clone, BorshDeserialize, BorshSerialize)]
pub struct DecodedEpochAccount {
    pub key: String,
    pub slot: u64,
    pub owner: String,
    pub decoded: Decoder,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct JsonEpochAccount {
    pub key: String,
    pub slot: u64,
    pub owner: String,
    pub decoded: serde_json::Value,
}
