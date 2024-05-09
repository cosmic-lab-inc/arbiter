use solana_client::rpc_response::RpcConfirmedTransactionStatusWithSignature;
use solana_sdk::{
    clock::{Slot, UnixTimestamp},
    pubkey::Pubkey,
    signature::Signature,
    transaction::Transaction,
};

pub struct TrxData {
    pub tx: Transaction,
    pub signature: Signature,
    pub signer: Pubkey,
    pub slot: Slot,
    pub block_time: UnixTimestamp,
}

pub struct SignatureInfo {
    pub ctx: RpcConfirmedTransactionStatusWithSignature,
    pub unix_seconds_since: i64,
    pub formatted_time_since: String,
}
