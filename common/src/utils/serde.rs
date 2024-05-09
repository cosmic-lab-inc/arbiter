use serde::{Deserialize, Deserializer};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

// Custom deserialization function for converting a String to a Pubkey
pub fn deserialize_pubkey<'de, D>(deserializer: D) -> Result<Pubkey, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Pubkey::from_str(&s).map_err(serde::de::Error::custom)
}

pub fn serialize_pubkey<S>(key: &Pubkey, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&key.to_string())
}

// deserialize Option<Pubkey>
pub fn deserialize_option_pubkey<'de, D>(deserializer: D) -> Result<Option<Pubkey>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = Option::<String>::deserialize(deserializer)?;
    match s {
        Some(s) => Ok(Some(
            Pubkey::from_str(&s).map_err(serde::de::Error::custom)?,
        )),
        None => Ok(None),
    }
}

// serialize Option<Pubkey>
pub fn serialize_option_pubkey<S>(key: &Option<Pubkey>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match key {
        Some(key) => serializer.serialize_str(&key.to_string()),
        None => serializer.serialize_none(),
    }
}
