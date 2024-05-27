#![allow(clippy::too_many_arguments)]

pub use anchor_gen::prelude::*;

pub use utils::*;

pub mod utils;

generate_cpi_crate!("idl.json");
declare_id!("dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH");

/// cargo test --package drift-cpi --lib accounts -- --exact --show-output
#[test]
fn accounts() {
  let idl_path = "idl.json";
  let idl_str = std::fs::read_to_string(idl_path).unwrap();
  let idl = serde_json::from_str::<serde_json::Value>(&idl_str).unwrap();
  let accounts = serde_json::from_value::<Vec<serde_json::Value>>(idl["accounts"].clone()).unwrap();
  for account in accounts {
    println!("{}", account["name"].as_str().unwrap());
  }
}

/// cargo test --package drift-cpi --lib instructions -- --exact --show-output
#[test]
fn instructions() {
  // let a = PlacePerpOrder {
  //
  // }
  // PlacePerpOrder::deserialize()

  let idl_path = "idl.json";
  let idl_str = std::fs::read_to_string(idl_path).unwrap();
  let idl = serde_json::from_str::<serde_json::Value>(&idl_str).unwrap();
  let ixs = serde_json::from_value::<Vec<serde_json::Value>>(idl["instructions"].clone()).unwrap();
  for ix in ixs {
    println!("{}", ix["name"].as_str().unwrap());
  }
}