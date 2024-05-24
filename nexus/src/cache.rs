use std::collections::HashMap;

use anchor_lang::AccountDeserialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;

use crate::{AcctCtx, DecodedAcctCtx, RingMap};
use crate::{DriftUtils, PerpOracle, SpotOracle};

#[derive(Debug, Hash, PartialEq, Eq)]
pub enum CacheKeyRegistry {
  PerpMarkets,
  SpotMarkets,
}

pub struct Cache {
  /// How many iterations back to keep in the cache
  pub depth: usize,
  pub slot: u64,
  pub accounts: HashMap<Pubkey, RingMap<u64, AcctCtx>>,
  pub key_registry: HashMap<CacheKeyRegistry, Vec<Pubkey>>,
}

impl Cache {
  pub fn new(depth: usize) -> Self {
    Self {
      slot: 0,
      depth,
      accounts: HashMap::new(),
      key_registry: HashMap::new(),
    }
  }

  pub fn slot(&self) -> u64 {
    self.slot
  }

  pub fn account(&self, key: &Pubkey, slot: Option<u64>) -> anyhow::Result<&AcctCtx> {
    let ring = self.accounts.get(key).ok_or(anyhow::anyhow!("Program not found for key: {}", key))?;
    Ok(match slot {
      Some(slot) => {
        ring.get(&slot).ok_or(anyhow::anyhow!("Slot not found for key: {}", key))?
      }
      None => ring.front().ok_or(anyhow::anyhow!("Slot not found for key: {}", key))?.1
    })
  }

  pub fn accounts(&self, slot: Option<u64>) -> anyhow::Result<Vec<&AcctCtx>> {
    let accts: Vec<&RingMap<u64, AcctCtx>> = self.accounts.values().collect();
    Ok(match slot {
      Some(slot) => {
        accts.iter().flat_map(|r| r.get(&slot)).collect()
      }
      None => {
        accts.iter().flat_map(|r| {
          r.front().map(|res| res.1)
        }).collect()
      }
    })
  }

  pub fn decoded_account<T: AccountDeserialize + Clone>(&self, key: &Pubkey, slot: Option<u64>) -> anyhow::Result<DecodedAcctCtx<T>> {
    let acct = self.account(key, slot)?;
    let decoded = T::try_deserialize(&mut acct.account.data.as_slice())?;
    Ok(DecodedAcctCtx {
      key: acct.key,
      account: acct.account.clone(),
      slot: acct.slot,
      decoded,
    })
  }

  pub fn registry_keys(&self, key: &CacheKeyRegistry, slot: Option<u64>) -> anyhow::Result<Vec<&AcctCtx>> {
    let mut accts = self.accounts(slot)?;
    let keys = self.key_registry.get(key).ok_or(anyhow::anyhow!("Key not found in registry"))?;
    accts.retain(|a| keys.contains(&a.key));
    Ok(accts)
  }

  pub fn registry_accounts<T: AccountDeserialize + Clone>(
    &self,
    key: &CacheKeyRegistry,
    slot: Option<u64>,
  ) -> anyhow::Result<Vec<DecodedAcctCtx<T>>> {
    let mut accts = self.accounts(slot)?;
    let keys = self.key_registry.get(key).ok_or(anyhow::anyhow!("Key not found in registry"))?;
    accts.retain(|a| keys.contains(&a.key));
    let res: Vec<DecodedAcctCtx<T>> = accts.into_iter().flat_map(|a| {
      let decoded = T::try_deserialize(&mut a.account.data.as_slice())?;
      Result::<_, anyhow::Error>::Ok(DecodedAcctCtx {
        key: a.key,
        account: a.account.clone(),
        slot: a.slot,
        decoded,
      })
    }).collect();
    Ok(res)
  }

  pub fn ring(&self, key: &Pubkey) -> anyhow::Result<&RingMap<u64, AcctCtx>> {
    self.accounts.get(key).ok_or(anyhow::anyhow!("RingMap not found for key: {}", key))
  }

  pub fn ring_mut(&mut self, key: Pubkey) -> &mut RingMap<u64, AcctCtx> {
    if self.accounts.get_mut(&key).is_none() {
      self.accounts.insert(key, RingMap::new(self.depth));
    }
    self.accounts.get_mut(&key).unwrap()
  }

  pub async fn load_all(
    &mut self,
    rpc: &RpcClient,
    users: &[Pubkey],
    accounts: &[Pubkey],
    auths: &[Pubkey],
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
      self.ring_mut(perp.key).insert(perp.slot, AcctCtx {
        key: perp.key,
        account: perp.account,
        slot: perp.slot,
      });
    }
    Ok(())
  }

  pub async fn load_spot_markets(&mut self, rpc: &RpcClient) -> anyhow::Result<()> {
    let spots = DriftUtils::spot_markets(rpc).await?;
    for spot in spots {
      self.ring_mut(spot.key).insert(spot.slot, AcctCtx {
        key: spot.key,
        account: spot.account,
        slot: spot.slot,
      });
    }
    Ok(())
  }

  pub async fn load_users(&mut self, rpc: &RpcClient, filter: &[Pubkey]) -> anyhow::Result<()> {
    let mut accts = DriftUtils::users(rpc).await?;
    accts.retain(|a| filter.contains(&a.key));
    for acct in accts {
      self.ring_mut(acct.key).insert(acct.slot, AcctCtx {
        key: acct.key,
        account: acct.account,
        slot: 0,
      });
    }
    Ok(())
  }

  pub async fn load_user_stats(&mut self, rpc: &RpcClient, auths: &[Pubkey]) -> anyhow::Result<()> {
    let accts = DriftUtils::user_stats(rpc, auths).await?;
    for ctx in accts {
      self.ring_mut(ctx.key).insert(ctx.slot, AcctCtx {
        key: ctx.key,
        account: ctx.account,
        slot: 0,
      });
    }
    Ok(())
  }

  pub async fn load_oracles(&mut self, rpc: &RpcClient) -> anyhow::Result<()> {
    let perp_markets = DriftUtils::perp_markets(rpc).await?;
    let spot_markets = DriftUtils::spot_markets(rpc).await?;
    let mut perp_oracles = HashMap::new();
    let mut spot_oracles = HashMap::new();

    for acct in perp_markets {
      let DecodedAcctCtx {
        decoded: perp_market,
        ..
      } = acct;
      let spot_market = spot_markets.iter().find(|spot| spot.decoded.market_index == perp_market.quote_spot_market_index).ok_or(anyhow::anyhow!("Spot market not found"))?;
      let spot_oracle = spot_market.decoded.oracle;
      let perp_oracle = perp_market.amm.oracle;

      perp_oracles.insert(perp_oracle, PerpOracle {
        source: perp_market.amm.oracle_source,
        market: perp_market,
      });
      spot_oracles.insert(spot_oracle, SpotOracle {
        source: spot_market.decoded.oracle_source,
        market: spot_market.decoded,
      });
    }

    let perp_oracle_keys = perp_oracles.keys().cloned().collect::<Vec<Pubkey>>();
    let perp_oracle_accts = rpc.get_multiple_accounts_with_commitment(
      &perp_oracle_keys,
      CommitmentConfig::confirmed(),
    ).await?;
    let slot = perp_oracle_accts.context.slot;

    let perp_oracle_accts: Vec<AcctCtx> = perp_oracle_accts.value.into_iter().enumerate().flat_map(|(i, a)| {
      match a {
        None => None,
        Some(account) => {
          let data = AcctCtx {
            key: perp_oracle_keys[i],
            account,
            slot,
          };
          Some(data)
        }
      }
    }).collect();
    for oracle in perp_oracle_accts {
      self.ring_mut(oracle.key).insert(oracle.slot, oracle);
    }

    let spot_oracle_keys = spot_oracles.keys().cloned().collect::<Vec<Pubkey>>();
    let spot_oracle_accts = rpc.get_multiple_accounts_with_commitment(
      &spot_oracle_keys,
      CommitmentConfig::confirmed(),
    ).await?;
    let slot = spot_oracle_accts.context.slot;

    let spot_oracle_accts: Vec<AcctCtx> = spot_oracle_accts.value.into_iter().enumerate().flat_map(|(i, a)| {
      match a {
        None => None,
        Some(account) => {
          let data = AcctCtx {
            key: spot_oracle_keys[i],
            account,
            slot,
          };
          Some(data)
        }
      }
    }).collect();
    for oracle in spot_oracle_accts {
      self.ring_mut(oracle.key).insert(oracle.slot, oracle);
    }

    Ok(())
  }

  pub async fn load_accounts(&mut self, rpc: &RpcClient, filter: &[Pubkey]) -> anyhow::Result<()> {
    let res = rpc.get_multiple_accounts_with_commitment(
      filter,
      CommitmentConfig::confirmed(),
    ).await?;
    let accts = res.value;
    let slot = res.context.slot;
    for (i, acct) in accts.into_iter().enumerate() {
      let key = filter[i];
      if let Some(account) = acct {
        self.ring_mut(key).insert(slot, AcctCtx {
          key,
          account,
          slot,
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