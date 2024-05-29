use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;

pub const TOKEN_PROGRAM_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
pub const DRIFT_API_PREFIX: &str = "https://drift-historical-data-v2.s3.eu-west-1.amazonaws.com/program/dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH/";
pub const PYTH_PROGRAM_ID: Pubkey = pubkey!("FsJ3A3u2vn5cTVofAjvy6y5kwABJAqYWpe4975bi2epH");

pub const MICRO_LAMPORTS_PER_LAMPORT: u64 = 1_000_000;
pub const LAMPORTS_PER_SOL: u64 = 1_000_000_000;
pub const MICRO_LAMPORTS_PER_SOL: u64 = LAMPORTS_PER_SOL * MICRO_LAMPORTS_PER_LAMPORT;
