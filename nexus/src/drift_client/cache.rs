use std::collections::HashMap;
use std::sync::Arc;

use anchor_lang::AccountDeserialize;
use drift_cpi::User;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client_api::config::RpcBlockConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::{Mutex, MutexGuard};

use crate::drift_client::{DriftUtils, PerpOracle, SpotOracle};
use crate::{AcctCtx, BlockInfo, DecodedAcctCtx, RingMap, Time};

#[derive(Debug, Hash, PartialEq, Eq)]
pub enum CacheKeyRegistry {
  PerpMarkets,
  SpotMarkets,
}

pub struct Cache {
  cache: Arc<Mutex<InnerCache>>,
}

impl Clone for Cache {
  fn clone(&self) -> Self {
    Self {
      cache: self.cache.clone(),
    }
  }
}

pub struct InnerCache {
  /// How many versions back to keep in the cache
  pub slot: u64,
  pub depth: usize,
  /// Key is slot
  pub blocks: RingMap<u64, BlockInfo>,
  pub accounts: HashMap<Pubkey, RingMap<u64, AcctCtx>>,
  pub key_registry: HashMap<CacheKeyRegistry, Vec<Pubkey>>,
}

// pub type ReadCache<'a> = RwLockReadGuard<'a, Cache>;
// pub type WriteCache<'a> = RwLockWriteGuard<'a, Cache>;
pub type ReadCache<'a> = MutexGuard<'a, InnerCache>;
pub type WriteCache<'a> = MutexGuard<'a, InnerCache>;

impl Cache {
  pub fn new(depth: usize) -> Self {
    Self {
      cache: Arc::new(Mutex::new(InnerCache {
        slot: 0,
        blocks: RingMap::new(depth),
        depth,
        accounts: HashMap::new(),
        key_registry: HashMap::new(),
      })),
    }
  }

  pub async fn read(&self) -> ReadCache {
    self.cache.lock().await
  }

  pub async fn write(&self) -> WriteCache {
    self.cache.lock().await
  }
}

impl InnerCache {
  pub fn block(&self, slot: Option<u64>) -> anyhow::Result<&BlockInfo> {
    Ok(match slot {
      Some(slot) => self
        .blocks
        .get(&slot)
        .ok_or(anyhow::anyhow!("Block not found for slot {}", slot))?,
      None => {
        self
          .blocks
          .newest()
          .ok_or(anyhow::anyhow!("Block not found"))?
          .1
      }
    })
  }

  fn take_closest_slot_for_account(
    &self,
    key: &Pubkey,
    slot: Option<u64>,
  ) -> anyhow::Result<&AcctCtx> {
    let ring = self
      .accounts
      .get(key)
      .ok_or(anyhow::anyhow!("Cache not found for key: {}", key))?;
    Ok(match slot {
      Some(slot) => match ring.get(&slot) {
        Some(acct) => acct,
        None => {
          let recent_updates = ring
            .values()
            .filter(|a| a.slot <= slot)
            .collect::<Vec<&AcctCtx>>();
          recent_updates.last().ok_or(anyhow::anyhow!(
            "Failed to get any slot <= {} for key: {}",
            slot,
            key
          ))?
        }
      },
      None => {
        ring
          .newest()
          .ok_or(anyhow::anyhow!("Slot not found for key: {}", key))?
          .1
      }
    })
  }

  #[allow(dead_code)]
  async fn wait_for_account(&self, key: &Pubkey, slot: Option<u64>) -> anyhow::Result<&AcctCtx> {
    Ok(match slot {
      Some(slot) => {
        match self
          .accounts
          .get(key)
          .ok_or(anyhow::anyhow!("Cache not found for key: {}", key))?
          .get(&slot)
        {
          Some(acct) => acct,
          None => {
            let mut not_found = true;
            let timeout = std::time::Duration::from_secs(5);
            let start = std::time::Instant::now();
            let mut acct: Option<&AcctCtx> = None;
            while not_found && start.elapsed() < timeout {
              let ring = self
                .accounts
                .get(key)
                .ok_or(anyhow::anyhow!("Cache not found for key: {}", key))?;
              tokio::time::sleep(std::time::Duration::from_millis(10)).await;
              if let Some(a) = ring.get(&slot) {
                acct = Some(a);
                not_found = false;
              }
            }
            match acct {
              None => {
                log::error!(
                  "After waiting slot {} still not found for key: {}",
                  slot,
                  key
                );
                Err(anyhow::anyhow!(
                  "After waiting slot {} still not found for key: {}",
                  slot,
                  key
                ))?
              }
              Some(acct) => acct,
            }
            // acct.ok_or(anyhow::anyhow!("After waiting slot {} still not found for key: {}", slot, key))?
          }
        }
      }
      None => {
        self
          .accounts
          .get(key)
          .ok_or(anyhow::anyhow!("Cache not found for key: {}", key))?
          .newest()
          .ok_or(anyhow::anyhow!("Slot not found for key: {}", key))?
          .1
      }
    })
  }

  pub fn account(&self, key: &Pubkey, slot: Option<u64>) -> anyhow::Result<&AcctCtx> {
    self.take_closest_slot_for_account(key, slot)
  }

  pub fn accounts(&self, slot: Option<u64>) -> anyhow::Result<Vec<&AcctCtx>> {
    let accts: Vec<&RingMap<u64, AcctCtx>> = self.accounts.values().collect();
    Ok(match slot {
      Some(slot) => accts.iter().flat_map(|r| r.get(&slot)).collect(),
      None => accts
        .iter()
        .flat_map(|r| r.newest().map(|res| res.1))
        .collect(),
    })
  }

  pub fn decoded_account<T: AccountDeserialize + Clone>(
    &self,
    key: &Pubkey,
    slot: Option<u64>,
  ) -> anyhow::Result<DecodedAcctCtx<T>> {
    let acct = self.account(key, slot)?;
    let decoded = T::try_deserialize(&mut acct.account.data.as_slice())?;
    Ok(DecodedAcctCtx {
      key: acct.key,
      account: acct.account.clone(),
      slot: acct.slot,
      decoded,
    })
  }

  pub fn decoded_accounts<T: AccountDeserialize + Clone>(
    &self,
    slot: Option<u64>,
  ) -> anyhow::Result<Vec<DecodedAcctCtx<T>>> {
    let accts: Vec<&RingMap<u64, AcctCtx>> = self.accounts.values().collect();
    let accts: Vec<DecodedAcctCtx<T>> = match slot {
      Some(slot) => accts
        .iter()
        .flat_map(|r| r.get(&slot))
        .flat_map(|a| self.decoded_account::<T>(&a.key, Some(slot)).ok())
        .collect(),
      None => accts
        .iter()
        .flat_map(|r| r.newest().map(|res| res.1))
        .flat_map(|a| self.decoded_account::<T>(&a.key, None).ok())
        .collect(),
    };
    Ok(accts)
  }

  pub fn registry_keys(
    &self,
    key: &CacheKeyRegistry,
    slot: Option<u64>,
  ) -> anyhow::Result<Vec<&AcctCtx>> {
    let mut accts = self.accounts(slot)?;
    let keys = self
      .key_registry
      .get(key)
      .ok_or(anyhow::anyhow!("Key not found in registry"))?;
    accts.retain(|a| keys.contains(&a.key));
    Ok(accts)
  }

  pub fn registry_accounts<T: AccountDeserialize + Clone>(
    &self,
    key: &CacheKeyRegistry,
    slot: Option<u64>,
  ) -> anyhow::Result<Vec<DecodedAcctCtx<T>>> {
    let mut accts = self.accounts(slot)?;
    let keys = self
      .key_registry
      .get(key)
      .ok_or(anyhow::anyhow!("Key not found in registry"))?;
    accts.retain(|a| keys.contains(&a.key));
    let res: Vec<DecodedAcctCtx<T>> = accts
      .into_iter()
      .flat_map(|a| {
        let decoded = T::try_deserialize(&mut a.account.data.as_slice())?;
        Result::<_, anyhow::Error>::Ok(DecodedAcctCtx {
          key: a.key,
          account: a.account.clone(),
          slot: a.slot,
          decoded,
        })
      })
      .collect();
    Ok(res)
  }

  pub fn ring(&self, key: &Pubkey) -> anyhow::Result<&RingMap<u64, AcctCtx>> {
    self
      .accounts
      .get(key)
      .ok_or(anyhow::anyhow!("RingMap not found for key: {}", key))
  }

  pub fn ring_mut(&mut self, key: Pubkey) -> &mut RingMap<u64, AcctCtx> {
    if self.accounts.get_mut(&key).is_none() {
      self.accounts.insert(key, RingMap::new(self.depth));
    }
    self.accounts.get_mut(&key).unwrap()
  }

  pub async fn load(
    &mut self,
    rpc: &RpcClient,
    users: &[Pubkey],
    accounts: Option<&[Pubkey]>,
    auths: &[Pubkey],
  ) -> anyhow::Result<()> {
    self.load_perp_markets(rpc).await?;
    self.load_spot_markets(rpc).await?;
    self.load_select_users_from_rpc(rpc, users).await?;
    self.load_user_stats(rpc, auths).await?;
    self.load_oracles(rpc).await?;
    if let Some(accounts) = accounts {
      self.load_accounts(rpc, accounts).await?;
    }
    self.load_block(rpc).await?;
    self.load_slot(rpc).await?;
    Ok(())
  }

  pub async fn load_with_all_users(
    &mut self,
    rpc: &RpcClient,
    users: Option<Vec<DecodedAcctCtx<User>>>,
    accounts: Option<&[Pubkey]>,
    auths: &[Pubkey],
  ) -> anyhow::Result<()> {
    self.load_perp_markets(rpc).await?;
    self.load_spot_markets(rpc).await?;
    self.load_users(rpc, users).await?;
    self.load_user_stats(rpc, auths).await?;
    self.load_oracles(rpc).await?;
    if let Some(accounts) = accounts {
      self.load_accounts(rpc, accounts).await?;
    }
    self.load_block(rpc).await?;
    self.load_slot(rpc).await?;
    Ok(())
  }

  async fn load_perp_markets(&mut self, rpc: &RpcClient) -> anyhow::Result<()> {
    let perps = DriftUtils::perp_markets(rpc).await?;
    for perp in perps {
      self.ring_mut(perp.key).insert(
        perp.slot,
        AcctCtx {
          key: perp.key,
          account: perp.account,
          slot: perp.slot,
        },
      );
    }
    Ok(())
  }

  async fn load_spot_markets(&mut self, rpc: &RpcClient) -> anyhow::Result<()> {
    let spots = DriftUtils::spot_markets(rpc).await?;
    for spot in spots {
      self.ring_mut(spot.key).insert(
        spot.slot,
        AcctCtx {
          key: spot.key,
          account: spot.account,
          slot: spot.slot,
        },
      );
    }
    Ok(())
  }

  async fn load_select_users_from_rpc(
    &mut self,
    rpc: &RpcClient,
    filter: &[Pubkey],
  ) -> anyhow::Result<()> {
    let res = rpc
      .get_multiple_accounts_with_commitment(filter, CommitmentConfig::processed())
      .await?;
    for (i, account) in res.value.into_iter().enumerate() {
      let key = filter[i];
      let account = account.ok_or(anyhow::anyhow!("Account not found for key: {}", key))?;
      self.ring_mut(key).insert(
        res.context.slot,
        AcctCtx {
          key,
          account,
          slot: res.context.slot,
        },
      );
    }
    Ok(())
  }

  async fn load_users(
    &mut self,
    rpc: &RpcClient,
    users: Option<Vec<DecodedAcctCtx<User>>>,
  ) -> anyhow::Result<()> {
    let users = match users {
      Some(users) => users,
      None => DriftUtils::users(rpc).await?,
    };
    for user in users {
      self.ring_mut(user.key).insert(
        user.slot,
        AcctCtx {
          key: user.key,
          account: user.account,
          slot: user.slot,
        },
      );
    }
    Ok(())
  }

  async fn load_user_stats(&mut self, rpc: &RpcClient, auths: &[Pubkey]) -> anyhow::Result<()> {
    let accts = DriftUtils::user_stats(rpc, auths).await?;
    for ctx in accts {
      self.ring_mut(ctx.key).insert(
        ctx.slot,
        AcctCtx {
          key: ctx.key,
          account: ctx.account,
          slot: 0,
        },
      );
    }
    Ok(())
  }

  async fn load_oracles(&mut self, rpc: &RpcClient) -> anyhow::Result<()> {
    let perp_markets = DriftUtils::perp_markets(rpc).await?;
    let spot_markets = DriftUtils::spot_markets(rpc).await?;
    let mut perp_oracles = HashMap::new();
    let mut spot_oracles = HashMap::new();

    for acct in perp_markets {
      let DecodedAcctCtx {
        decoded: perp_market,
        ..
      } = acct;
      let spot_market = spot_markets
        .iter()
        .find(|spot| spot.decoded.market_index == perp_market.quote_spot_market_index)
        .ok_or(anyhow::anyhow!("Spot market not found"))?;
      let spot_oracle = spot_market.decoded.oracle;
      let perp_oracle = perp_market.amm.oracle;

      perp_oracles.insert(
        perp_oracle,
        PerpOracle {
          source: perp_market.amm.oracle_source,
          market: perp_market,
        },
      );
      spot_oracles.insert(
        spot_oracle,
        SpotOracle {
          source: spot_market.decoded.oracle_source,
          market: spot_market.decoded,
        },
      );
    }

    let perp_oracle_keys = perp_oracles.keys().cloned().collect::<Vec<Pubkey>>();
    let perp_oracle_accts = rpc
      .get_multiple_accounts_with_commitment(&perp_oracle_keys, CommitmentConfig::confirmed())
      .await?;
    let slot = perp_oracle_accts.context.slot;

    let perp_oracle_accts: Vec<AcctCtx> = perp_oracle_accts
      .value
      .into_iter()
      .enumerate()
      .flat_map(|(i, a)| match a {
        None => None,
        Some(account) => {
          let data = AcctCtx {
            key: perp_oracle_keys[i],
            account,
            slot,
          };
          Some(data)
        }
      })
      .collect();
    for oracle in perp_oracle_accts {
      self.ring_mut(oracle.key).insert(oracle.slot, oracle);
    }

    let spot_oracle_keys = spot_oracles.keys().cloned().collect::<Vec<Pubkey>>();
    let spot_oracle_accts = rpc
      .get_multiple_accounts_with_commitment(&spot_oracle_keys, CommitmentConfig::confirmed())
      .await?;
    let slot = spot_oracle_accts.context.slot;

    let spot_oracle_accts: Vec<AcctCtx> = spot_oracle_accts
      .value
      .into_iter()
      .enumerate()
      .flat_map(|(i, a)| match a {
        None => None,
        Some(account) => {
          let data = AcctCtx {
            key: spot_oracle_keys[i],
            account,
            slot,
          };
          Some(data)
        }
      })
      .collect();
    for oracle in spot_oracle_accts {
      self.ring_mut(oracle.key).insert(oracle.slot, oracle);
    }

    Ok(())
  }

  async fn load_accounts(&mut self, rpc: &RpcClient, filter: &[Pubkey]) -> anyhow::Result<()> {
    let res = rpc
      .get_multiple_accounts_with_commitment(filter, CommitmentConfig::confirmed())
      .await?;
    let accts = res.value;
    let slot = res.context.slot;
    for (i, acct) in accts.into_iter().enumerate() {
      let key = filter[i];
      if let Some(account) = acct {
        self
          .ring_mut(key)
          .insert(slot, AcctCtx { key, account, slot });
      }
    }
    Ok(())
  }

  async fn load_block(&mut self, rpc: &RpcClient) -> anyhow::Result<()> {
    let slot = rpc
      .get_slot_with_commitment(CommitmentConfig::confirmed())
      .await?;
    let config = RpcBlockConfig {
      encoding: Some(solana_transaction_status::UiTransactionEncoding::JsonParsed),
      transaction_details: Some(solana_transaction_status::TransactionDetails::None),
      rewards: Some(false),
      commitment: Some(CommitmentConfig::confirmed()),
      max_supported_transaction_version: Some(1),
    };
    let block = rpc.get_block_with_config(slot, config).await?;
    if let Some(timestamp) = block.block_time {
      self.blocks.insert(
        slot,
        BlockInfo {
          slot,
          blockhash: block.blockhash,
          time: Time::from_unix(timestamp),
        },
      );
    }
    Ok(())
  }

  async fn load_slot(&mut self, rpc: &RpcClient) -> anyhow::Result<()> {
    let slot = rpc
      .get_slot_with_commitment(CommitmentConfig::confirmed())
      .await?;
    self.slot = slot;
    Ok(())
  }
}
