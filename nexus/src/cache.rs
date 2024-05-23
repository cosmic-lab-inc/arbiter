use std::collections::HashMap;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::account::Account;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;

use crate::{AccountContext, DecodedAccountContext};
use crate::{DriftUtils, PerpOracle, SpotOracle};
use crate::drift_cpi::*;

#[derive(Default)]
pub struct Cache {
  pub perp_markets: HashMap<Pubkey, DecodedAccountContext<PerpMarket>>,
  pub spot_markets: HashMap<Pubkey, DecodedAccountContext<SpotMarket>>,
  pub users: HashMap<Pubkey, DecodedAccountContext<User>>,
  pub user_stats: HashMap<Pubkey, DecodedAccountContext<UserStats>>,
  pub perp_oracles: HashMap<Pubkey, DecodedAccountContext<PerpOracle>>,
  pub spot_oracles: HashMap<Pubkey, DecodedAccountContext<SpotOracle>>,
  pub accounts: HashMap<Pubkey, AccountContext<Account>>,
  pub slot: u64
}

impl Cache {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn find_perp_market(&self, key: &Pubkey) -> anyhow::Result<&DecodedAccountContext<PerpMarket>> {
    self.perp_markets.get(key).ok_or(anyhow::anyhow!("PerpMarket not found for key: {}", key))
  }
  pub fn perp_markets(&self) -> Vec<&DecodedAccountContext<PerpMarket>> {
    self.perp_markets.values().collect()
  }

  pub fn find_spot_market(&self, key: &Pubkey) -> anyhow::Result<&DecodedAccountContext<SpotMarket>> {
    self.spot_markets.get(key).ok_or(anyhow::anyhow!("SpotMarket not found for key: {}", key))
  }
  pub fn spot_markets(&self) -> Vec<&DecodedAccountContext<SpotMarket>> {
    self.spot_markets.values().collect()
  }

  pub fn find_user(&self, key: &Pubkey) -> anyhow::Result<&DecodedAccountContext<User>> {
    self.users.get(key).ok_or(anyhow::anyhow!("User not found for key: {}", key))
  }
  pub fn users(&self) -> Vec<&DecodedAccountContext<User>> {
    self.users.values().collect()
  }

  pub fn find_user_stats(&self, key: &Pubkey) -> anyhow::Result<&DecodedAccountContext<User>> {
    self.users.get(key).ok_or(anyhow::anyhow!("User not found for key: {}", key))
  }
  pub fn users_stats(&self) -> Vec<&DecodedAccountContext<User>> {
    self.users.values().collect()
  }

  pub fn find_perp_oracle(&self, key: &Pubkey) -> anyhow::Result<&DecodedAccountContext<PerpOracle>> {
    self.perp_oracles.get(key).ok_or(anyhow::anyhow!("PerpOracle not found for key: {}", key))
  }
  pub fn perp_oracles(&self) -> Vec<&DecodedAccountContext<PerpOracle>> {
    self.perp_oracles.values().collect()
  }

  pub fn find_spot_oracle(&self, key: &Pubkey) -> anyhow::Result<&DecodedAccountContext<SpotOracle>> {
    self.spot_oracles.get(key).ok_or(anyhow::anyhow!("SpotOracle not found for key: {}", key))
  }
  pub fn spot_oracles(&self) -> Vec<&DecodedAccountContext<SpotOracle>> {
    self.spot_oracles.values().collect()
  }

  pub fn find_account(&self, key: &Pubkey) -> anyhow::Result<&AccountContext<Account>> {
    self.accounts.get(key).ok_or(anyhow::anyhow!("Program not found for key: {}", key))
  }
  pub fn accounts(&self) -> Vec<&AccountContext<Account>> {
    self.accounts.values().collect()
  }

  pub fn slot(&self) -> u64 {
    self.slot
  }


  pub async fn load_all(
    &mut self,
    rpc: &RpcClient,
    users: &[Pubkey],
    accounts: &[Pubkey],
    auths: &[Pubkey]
  ) -> anyhow::Result<()> {
    self.load_perp_markets(rpc).await?;
    self.load_spot_markets(rpc).await?;
    self.load_users(rpc, users).await?;
    self.load_user_stats(rpc, auths).await?;
    self.load_oracles(rpc).await?;
    self.load_accounts(rpc, accounts).await?;
    self.load_slot(rpc).await?;
    Ok(())
  }

  pub async fn load_perp_markets(&mut self, rpc: &RpcClient) -> anyhow::Result<()> {
    let perps = DriftUtils::perp_markets(rpc).await?;
    for perp in perps {
      self.perp_markets.insert(perp.key, perp);
    }
    Ok(())
  }

  pub async fn load_spot_markets(&mut self, rpc: &RpcClient) -> anyhow::Result<()> {
    let spots = DriftUtils::spot_markets(rpc).await?;
    for spot in spots {
      self.spot_markets.insert(spot.key, spot);
    }
    Ok(())
  }

  pub async fn load_users(&mut self, rpc: &RpcClient, filter: &[Pubkey]) -> anyhow::Result<()> {
    let mut accts = DriftUtils::users(rpc).await?;
    accts.retain(|a| filter.contains(&a.key));
    for acct in accts {
      let raw = AccountType::decode(acct.account.data.as_slice()).map_err(
        |e| anyhow::anyhow!("Failed to decode account: {:?}", e)
      )?;
      if let AccountType::User(decoded) = raw {
        self.users.insert(acct.key, DecodedAccountContext {
          key: acct.key,
          account: acct.account,
          slot: 0,
          decoded,
        });
      }
    }
    Ok(())
  }

  pub async fn load_user_stats(&mut self, rpc: &RpcClient, auths: &[Pubkey]) -> anyhow::Result<()> {
    let accts = DriftUtils::user_stats(rpc, auths).await?;
    for ctx in accts {
      let acct = AccountType::decode(ctx.account.data.as_slice()).map_err(
        |e| anyhow::anyhow!("Failed to decode account: {:?}", e)
      )?;
      if let AccountType::User(decoded) = acct {
        self.users.insert(ctx.key, DecodedAccountContext {
          key: ctx.key,
          account: ctx.account,
          slot: 0,
          decoded,
        });
      }
    }
    Ok(())
  }

  pub async fn load_oracles(&mut self, rpc: &RpcClient) -> anyhow::Result<()> {
    let perp_markets = DriftUtils::perp_markets(rpc).await?;
    let spot_markets = DriftUtils::spot_markets(rpc).await?;
    let mut perp_oracles = HashMap::new();
    let mut spot_oracles = HashMap::new();

    for acct in perp_markets {
      let DecodedAccountContext {
        decoded: perp_market,
        ..
      } = acct;
      let spot_market = spot_markets
        .iter()
        .find(|spot| spot.decoded.market_index == perp_market.quote_spot_market_index)
        .ok_or(anyhow::anyhow!("Spot market not found"))?;
      let spot_oracle = spot_market.decoded.oracle;
      let perp_oracle = perp_market.amm.oracle;

      perp_oracles.insert(perp_oracle, PerpOracle {
        source: perp_market.amm.oracle_source,
        market: perp_market
      });
      spot_oracles.insert(spot_oracle, SpotOracle {
        source: spot_market.decoded.oracle_source,
        market: spot_market.decoded
      });
    }

    let perp_oracle_keys = perp_oracles.keys().cloned().collect::<Vec<Pubkey>>();
    let perp_oracle_accts = rpc.get_multiple_accounts_with_commitment(
      &perp_oracle_keys,
      CommitmentConfig::confirmed()
    ).await?;
    let slot = perp_oracle_accts.context.slot;

    let perp_oracle_accts: Vec<DecodedAccountContext<PerpOracle>> = perp_oracle_accts.value.into_iter().enumerate().flat_map(|(i, a)| {
      match a {
        None => None,
        Some(account) => {
          let data = DecodedAccountContext {
            key: perp_oracle_keys[i],
            account,
            slot,
            decoded: perp_oracles.get(&perp_oracle_keys[i]).unwrap().clone()
          };
          Some(data)
        },
      }
    }).collect();
    for oracle in perp_oracle_accts {
      self.perp_oracles.insert(oracle.key, oracle);
    }

    let spot_oracle_keys = spot_oracles.keys().cloned().collect::<Vec<Pubkey>>();
    let spot_oracle_accts = rpc.get_multiple_accounts_with_commitment(
      &spot_oracle_keys,
      CommitmentConfig::confirmed()
    ).await?;
    let slot = spot_oracle_accts.context.slot;

    let spot_oracle_accts: Vec<DecodedAccountContext<SpotOracle>> = spot_oracle_accts.value.into_iter().enumerate().flat_map(|(i, a)| {
      match a {
        None => None,
        Some(account) => {
          let data = DecodedAccountContext {
            key: spot_oracle_keys[i],
            account,
            slot,
            decoded: spot_oracles.get(&spot_oracle_keys[i]).unwrap().clone()
          };
          Some(data)
        },
      }
    }).collect();
    for oracle in spot_oracle_accts {
      self.spot_oracles.insert(oracle.key, oracle);
    }

    Ok(())
  }

  pub async fn load_accounts(&mut self, rpc: &RpcClient, filter: &[Pubkey]) -> anyhow::Result<()> {
    let res = rpc.get_multiple_accounts_with_commitment(
      filter,
      CommitmentConfig::confirmed()
    ).await?;
    let accts = res.value;
    let slot = res.context.slot;
    for (i, acct) in accts.into_iter().enumerate() {
      let key = filter[i];
      if let Some(account) = acct {
        self.accounts.insert(key, AccountContext {
          key,
          account,
          slot
        });
      }
    }
    Ok(())
  }

  pub async fn load_slot(&mut self, rpc: &RpcClient) -> anyhow::Result<()> {
    let slot = rpc.get_slot().await?;
    self.slot = slot;
    Ok(())
  }
}