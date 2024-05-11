// use crate::drift::perp::PerpMarket;
// use crate::drift::spot::{SpotBalanceType, SpotMarket};
use drift::state::{
  perp_market::PerpMarket,
  spot_market::{SpotBalanceType, SpotMarket},
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;

use crate::Decoder;

pub struct TokenBalance {
  pub balance: u128,
  pub mint: Pubkey,
}

pub struct Drift;

impl Drift {
  pub fn decode_name(name: &[u8; 32]) -> String {
    String::from_utf8(name.to_vec()).unwrap().trim().to_string()
  }

  pub fn user_pda(authority: &Pubkey, sub_account_id: u16) -> anyhow::Result<Pubkey> {
    let seeds: &[&[u8]] = &[
      b"user",
      &authority.to_bytes()[..],
      &sub_account_id.to_le_bytes(),
    ];
    Ok(Pubkey::find_program_address(seeds, &drift::id()).0)
  }

  pub fn user_stats_pda(authority: &Pubkey) -> anyhow::Result<Pubkey> {
    let seeds: &[&[u8]] = &[b"user_stats", &authority.to_bytes()[..]];
    Ok(Pubkey::find_program_address(seeds, &drift::id()).0)
  }

  pub fn spot_market_pda(market_index: u16) -> anyhow::Result<Pubkey> {
    let seeds: &[&[u8]] = &[b"spot_market", &market_index.to_le_bytes()];
    Ok(Pubkey::find_program_address(seeds, &drift::id()).0)
  }

  pub fn perp_market_pda(market_index: u16) -> anyhow::Result<Pubkey> {
    let seeds: &[&[u8]] = &[b"perp_market", &market_index.to_le_bytes()];
    Ok(Pubkey::find_program_address(seeds, &drift::id()).0)
  }

  /// token_amount = SpotPosition.scaled_balance as u128
  ///
  /// SpotMarket = fetch SpotMarkets from Epoch and find where
  /// spot_market = SpotMarket.market_index == SpotPosition.market_index
  ///
  /// balance_type = SpotPosition.balance_type
  pub fn spot_balance(
    token_amount: u128,
    spot_market: &SpotMarket,
    balance_type: &SpotBalanceType,
    round_up: bool,
  ) -> anyhow::Result<TokenBalance> {
    let precision_increase = 10_u128.pow(
      19_u32
        .checked_sub(spot_market.decimals)
        .ok_or(anyhow::anyhow!("Checked sub overflow"))?,
    );

    let cumulative_interest = match balance_type {
      SpotBalanceType::Deposit => spot_market.cumulative_deposit_interest,
      SpotBalanceType::Borrow => spot_market.cumulative_borrow_interest,
    };

    let mut balance = token_amount
      .checked_mul(precision_increase)
      .ok_or(anyhow::anyhow!("Checked mul overflow"))?
      .checked_div(cumulative_interest)
      .ok_or(anyhow::anyhow!("Checked div overflow"))?;

    if round_up && balance != 0 {
      balance = balance
        .checked_add(1)
        .ok_or(anyhow::anyhow!("Checked add overflow"))?;
    }

    Ok(TokenBalance {
      balance,
      mint: spot_market.mint,
    })
  }

  pub async fn perp_markets(client: &RpcClient) -> anyhow::Result<Vec<PerpMarket>> {
    let pdas = (0..50)
      .flat_map(|i| Self::perp_market_pda(i as u16))
      .collect::<Vec<Pubkey>>();

    let keyed_accounts = client.get_multiple_accounts(&pdas).await?;
    let valid_accounts: Vec<Account> = keyed_accounts.into_iter().flatten().collect();
    let markets: Vec<PerpMarket> = valid_accounts
      .into_iter()
      .flat_map(|a| {
        let bytes = &a.data.as_slice()[8..];
        let acct = crate::drift::perp::PerpMarket::try_from_slice(bytes)?;
        match Decoder::de::<PerpMarket>(bytes) {
          Ok(market) => Some(*market),
          Err(_) => None,
        }
      })
      .collect();
    Ok(markets)
  }

  pub async fn spot_markets(client: &RpcClient) -> anyhow::Result<Vec<SpotMarket>> {
    let pdas = (0..50)
      .flat_map(|i| Self::spot_market_pda(i as u16))
      .collect::<Vec<Pubkey>>();

    let keyed_accounts = client.get_multiple_accounts(&pdas).await?;
    let valid_accounts: Vec<Account> = keyed_accounts.into_iter().flatten().collect();
    let markets: Vec<SpotMarket> = valid_accounts
      .into_iter()
      .flat_map(|a| {
        let bytes = &a.data.as_slice()[8..];
        match Decoder::de::<SpotMarket>(bytes) {
          Ok(market) => Some(*market),
          Err(_) => None,
        }
      })
      .collect();
    Ok(markets)
  }
}
