#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(clippy::too_many_arguments)]

use once_cell::sync::Lazy;

use common::decode_account;

pub mod oracle;
pub mod math;
pub mod casting;
pub mod safe_math;
pub mod safe_unwrap;
pub mod ceil_div;
pub mod floor_div;

anchor_gen::generate_cpi_crate!("idl.json");
anchor_lang::declare_id!("dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH");

pub static PATH: Lazy<String> = Lazy::new(|| env!("CARGO_MANIFEST_DIR").to_string());
pub static PROGRAM_NAME: Lazy<String> = Lazy::new(|| PATH.split('/').last().unwrap().to_string());
pub static PROGRAM_ID: Lazy<Pubkey> = Lazy::new(|| ID);

decode_account!(
    pub enum AccountType {
        PhoenixV1FulfillmentConfig(PhoenixV1FulfillmentConfig),
        SerumV3FulfillmentConfig(SerumV3FulfillmentConfig),
        InsuranceFundStake(InsuranceFundStake),
        ProtocolIfSharesTransferConfig(ProtocolIfSharesTransferConfig),
        PerpMarket(PerpMarket),
        SpotMarket(SpotMarket),
        State(State),
        User(User),
        UserStats(UserStats),
        ReferrerName(ReferrerName),
    }
);

/// cargo test --package drift-cpi --lib accounts -- --exact --show-output
#[test]
fn accounts() {
  let idl_path = "idl.json";
  let idl_str = std::fs::read_to_string(idl_path).unwrap();
  let idl = serde_json::from_str::<serde_json::Value>(&idl_str).unwrap();
  let accounts = serde_json::from_value::<Vec<serde_json::Value>>(idl["accounts"].clone()).unwrap();
  for account in accounts {
    println!("{}", account["name"].as_str().unwrap().to_string());
  }
}