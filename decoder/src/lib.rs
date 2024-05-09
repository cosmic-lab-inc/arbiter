pub use decoded_account::*;
pub use drift_cpi;
pub use program_decoder::*;
pub use program_helpers::*;

pub mod decoded_account;
pub mod program_decoder;
pub mod program_helpers;

#[test]
fn drift_user_discrim_to_base64() {
  use base64::engine::general_purpose;
  use base64::Engine;

  let bytes = ProgramDecoder::account_discriminator("User");
  let data = general_purpose::STANDARD.encode(bytes);
  assert_eq!(data, "n3Vf4++XOuw=");
  println!("\"User\" as base64: {}", data);
}
