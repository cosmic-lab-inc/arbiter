use solana_sdk::signature::Keypair;

pub fn read_keypair_from_env(env_var: &str) -> anyhow::Result<Keypair> {
  let raw_mint = std::env::var(env_var)
    .map_err(|e| anyhow::anyhow!("Failed to get {} from env: {}", env_var, e))?;
  let raw: Vec<u8> = raw_mint
    .trim_matches(|c| c == '[' || c == ']') // Remove the square brackets
    .split(',') // Split the string into an iterator of substrings based on the comma
    .filter_map(|s| s.trim().parse().ok()) // Parse each substring to u8, filtering out any errors
    .collect(); // Collect the values into a Vec<u8>
  Ok(Keypair::from_bytes(&raw)?)
}