use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;

pub const MARKET_LOOKUP_TABLE: Pubkey = pubkey!("D9cnvzswDikQDf53k4HpQ3KJ9y1Fv3HGGDFYMXnK5T6c");
pub const QUOTE_PRECISION: u128 = 1_000_000; // expo = -6
pub const PRICE_PRECISION: u128 = 1_000_000; //expo = -6;
pub const PRICE_PRECISION_I64: i64 = 1_000_000; //expo = -6;
pub const BASE_PRECISION: u64 = 1_000_000_000; //expo = -9;
pub const QUOTE_SPOT_MARKET_INDEX: u16 = 0; // USDC spot market index
pub const QUOTE_SPOT_MARKET_MINT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
pub const SOL_PERP_MARKET_INDEX: u16 = 0;
pub const SOL_SPOT_MARKET_INDEX: u16 = 1;
pub const SOL_SPOT_MARKET_MINT: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
pub const MIN_SOL_BALANCE: f64 = 0.1;