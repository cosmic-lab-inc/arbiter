#![allow(unused_imports)]

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::{Deref, Neg};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anchor_lang::context::Context;
use anchor_lang::prelude::{AccountInfo, AccountMeta, CpiContext};
use anchor_lang::{Accounts, Bumps, Discriminator, InstructionData, ToAccountMetas};
use base64::engine::general_purpose;
use base64::Engine;
use borsh::BorshDeserialize;
use crossbeam::channel::{Receiver, Sender};
use futures::StreamExt;
use log::{debug, error, info, warn};
use reqwest::Client;
use solana_account_decoder::{UiAccount, UiAccountData, UiAccountEncoding};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::{
  RpcAccountInfoConfig, RpcProgramAccountsConfig, RpcSimulateTransactionAccountsConfig,
  RpcSimulateTransactionConfig,
};
use solana_client::rpc_filter::{Memcmp, RpcFilterType};
use solana_client::rpc_response::RpcKeyedAccount;
use solana_sdk::account::Account;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::slot_hashes::SlotHashes;
use solana_sdk::sysvar::SysvarId;
use solana_sdk::transaction::Transaction;
use solana_transaction_status::UiTransactionEncoding;
use tokio::io::ReadBuf;
use tokio::sync::RwLockReadGuard;
use tokio::sync::{RwLock, RwLockWriteGuard};
use tokio::time::Instant;
use yellowstone_grpc_proto::prelude::subscribe_request_filter_accounts_filter::Filter;
use yellowstone_grpc_proto::prelude::{
  subscribe_request_filter_accounts_filter_memcmp, CommitmentLevel, SubscribeRequestFilterAccounts,
  SubscribeRequestFilterAccountsFilter, SubscribeRequestFilterAccountsFilterMemcmp,
  SubscribeRequestFilterBlocks, SubscribeRequestFilterBlocksMeta, SubscribeRequestFilterSlots,
  SubscribeRequestFilterTransactions,
};

use crate::config::UncrossConfig;
use nexus::drift_client::*;
use nexus::drift_cpi::{Decode, DiscrimToName, InstructionType, MarketType};
use nexus::*;

pub struct Uncross {
  read_only: bool,
  retry_until_confirmed: bool,
  pub signer: Arc<Keypair>,
  pub rpc: Arc<RpcClient>,
  pub drift: DriftClient,
  pub market: MarketId,
  pub cache: Cache,
  pub orderbook: Orderbook,
  pct_spread_multiplier: f64,
  pct_stop_loss: f64,
  leverage: f64,
  pct_max_spread: f64,
}

impl Uncross {
  pub async fn new(
    sub_account_id: u16,
    market: MarketId,
    cache_depth: Option<usize>,
  ) -> anyhow::Result<Self> {
    let UncrossConfig {
      read_only,
      retry_until_confirmed,
      signer,
      rpc_url,
      grpc,
      x_token,
      pct_spread_multiplier,
      pct_stop_loss,
      leverage,
      pct_max_spread,
      ..
    } = UncrossConfig::read()?;

    // 200 slots = 80 seconds of account cache
    let cache_depth = cache_depth.unwrap_or(200);
    let signer = Arc::new(signer);
    info!("Uncross using wallet: {}", signer.pubkey());
    let rpc = Arc::new(RpcClient::new_with_timeout(
      rpc_url,
      Duration::from_secs(90),
    ));

    let now = Instant::now();
    let users = DriftUtils::users(&rpc).await?;
    let orderbook = Orderbook::new(vec![market], &users).await?;
    info!("orderbook loaded in {:?}", now.elapsed());

    let this = Self {
      read_only,
      retry_until_confirmed,
      drift: DriftClient::new(
        signer.clone(),
        rpc.clone(),
        sub_account_id,
        None,
        read_only,
        retry_until_confirmed,
      )
      .await?,
      rpc,
      signer,
      cache: Cache::new(cache_depth),
      orderbook,
      market,
      pct_spread_multiplier,
      pct_stop_loss,
      leverage,
      pct_max_spread,
    };

    let account_filter = this.account_filter(users).await?;
    let mut filter = HashSet::new();
    for a in account_filter {
      filter.insert(a);
    }
    let cfg = this.orderbook_geyser_config(grpc, x_token)?;
    // stream updates from gRPC
    let nexus = NexusClient::new(cfg)?;
    let cache = this.cache.clone();
    let orderbook = this.orderbook.clone();
    tokio::task::spawn(async move {
      nexus
        .stream(&cache, None, Some(&orderbook), Some(filter))
        .await?;
      Result::<_, anyhow::Error>::Ok(())
    });
    Ok(this)
  }

  pub fn rpc(&self) -> Arc<RpcClient> {
    self.rpc.clone()
  }
  pub async fn cache(&self) -> ReadCache {
    self.cache.read().await
  }
  pub fn user(&self) -> &Pubkey {
    &self.drift.sub_account
  }
  pub async fn orderbook(&self) -> ReadOrderbook {
    self.orderbook.read().await
  }

  fn geyser_config(
    &self,
    grpc: String,
    x_token: String,
    account_filter: Vec<Pubkey>,
  ) -> anyhow::Result<GeyserConfig> {
    Ok(GeyserConfig {
      grpc,
      x_token,
      slots: Some(SubscribeRequestFilterSlots {
        filter_by_commitment: Some(true),
      }),
      accounts: Some(SubscribeRequestFilterAccounts {
        account: account_filter.into_iter().map(|k| k.to_string()).collect(),
        owner: vec![],
        filters: vec![],
      }),
      transactions: None,
      blocks_meta: None,
      commitment: CommitmentLevel::Processed,
    })
  }

  fn orderbook_geyser_config(&self, grpc: String, x_token: String) -> anyhow::Result<GeyserConfig> {
    let drift_program_id = id();
    Ok(GeyserConfig {
      grpc,
      x_token,
      slots: Some(SubscribeRequestFilterSlots {
        filter_by_commitment: Some(true),
      }),
      accounts: Some(SubscribeRequestFilterAccounts {
        account: vec![],
        owner: vec![drift_program_id.to_string(), PYTH_PROGRAM_ID.to_string()],
        filters: vec![],
      }),
      transactions: None,
      blocks_meta: None,
      commitment: CommitmentLevel::Processed,
    })
  }

  pub async fn start(&mut self) -> anyhow::Result<()> {
    self.drift.setup_user().await?;
    self.reset().await?;
    let run = AtomicBool::new(true);

    let mut last_update = Instant::now();
    while run.load(Ordering::Relaxed) {
      let user = self
        .cache()
        .await
        .decoded_account::<User>(self.user(), None)?
        .decoded;

      let mut long_order = user.orders.iter().find(|o| {
        MarketId::from((o.market_index, o.market_type)) == self.market
          && matches!(o.direction, PositionDirection::Long)
      });
      let mut short_order = user.orders.iter().find(|o| {
        MarketId::from((o.market_index, o.market_type)) == self.market
          && matches!(o.direction, PositionDirection::Short)
      });
      if Self::blank_order(long_order) {
        long_order = None;
      }
      if Self::blank_order(short_order) {
        short_order = None;
      }

      let pos = user
        .perp_positions
        .iter()
        .find(|pos| MarketId::perp(pos.market_index) == self.market);

      let (long_pos, short_pos) = match pos {
        Some(pos) => match pos.base_asset_amount {
          x if x > 0 => (Some(pos), None),
          x if x < 0 => (None, Some(pos)),
          _ => (None, None),
        },
        None => (None, None),
      };

      match (long_order, short_order, long_pos, short_pos) {
        (Some(lo), Some(so), None, None) => {
          let market_info = self
            .drift
            .market_info(self.market, &self.cache().await, None)?;
          let long_price = lo.price as f64 / PRICE_PRECISION as f64;
          let long_price_pct_diff = market_info.price / long_price * 100.0 - 100.0;
          let short_price = so.price as f64 / PRICE_PRECISION as f64;
          let short_price_pct_diff = market_info.price / short_price * 100.0 - 100.0;

          let long_filled = matches!(lo.status, OrderStatus::Filled);
          let short_filled = matches!(so.status, OrderStatus::Filled);

          if long_filled && short_filled {
            let pnl = Self::pct_pnl(lo, so);
            info!("🟢 pnl: {}%", trunc!(pnl, 4));
          } else if short_price_pct_diff > self.pct_stop_loss
            || long_price_pct_diff < self.pct_stop_loss * -1.0
          {
            info!(
              "🔴 price moved {}% beyond spread orders, reset position",
              self.pct_stop_loss
            );
            self.reset().await?;
          }
        }
        (Some(_), None, None, Some(spos)) => {
          let market_info = self
            .drift
            .market_info(self.market, &self.cache().await, None)?;

          let short_price = DriftUtils::perp_position_price(spos);
          debug!("short pos: ${}", trunc!(short_price, 3));

          let pct_diff = market_info.price / short_price * 100.0 - 100.0;
          // if price moves far above the ask, the bid likely won't fill
          if pct_diff > self.pct_stop_loss {
            info!(
              "🔴 price moved {}% above short entry, reset position",
              self.pct_stop_loss
            );
            self.reset().await?;
          }
        }
        (None, Some(_), Some(lpos), None) => {
          let market_info = self
            .drift
            .market_info(self.market, &self.cache().await, None)?;

          let long_price = DriftUtils::perp_position_price(lpos);
          debug!("long pos: ${}", trunc!(long_price, 3));

          let pct_diff = market_info.price / long_price * 100.0 - 100.0;
          // if price moves far below the bid, the ask likely won't fill
          if pct_diff < self.pct_stop_loss.neg() {
            info!(
              "🔴 price moved -{}% below long entry, reset position",
              self.pct_stop_loss
            );
            self.reset().await?;
          }
        }
        (None, None, Some(lpos), Some(spos)) => {
          let lp = DriftUtils::perp_position_price(lpos);
          let sp = DriftUtils::perp_position_price(spos);
          info!("🟢 pnl: {}%", trunc!(sp / lp * 100.0 - 100.0, 4));
        }
        (None, None, None, None) => {
          // no open orders or positions, place new orders
          let mut trx = self.new_tx();
          self
            .place_orders(self.build_orders().await?, &mut trx)
            .await?;
          trx.send_tx(id(), None).await?;
        }
        _ => {}
      }

      if last_update.elapsed() > Duration::from_secs(60 * 2) {
        info!("🔴 no activity for 2 minutes, reset position");
        self.reset().await?;
      }

      tokio::time::sleep(Duration::from_millis(400)).await;
      last_update = Instant::now();
    }

    Ok(())
  }

  fn pct_pnl(long: &Order, short: &Order) -> f64 {
    let lp = DriftUtils::order_price(long);
    let sp = DriftUtils::order_price(short);
    trunc!(sp / lp * 100.0 - 100.0, 4)
  }

  fn blank_order(order: Option<&Order>) -> bool {
    match order {
      Some(o) => o.base_asset_amount == 0,
      None => true,
    }
  }

  async fn reset(&self) -> anyhow::Result<()> {
    let mut trx = self.new_tx();
    trx = trx.retry_until_confirmed();
    self.cancel_orders(None, None, &mut trx).await?;
    self.close_positions(&[self.market], &mut trx).await?;
    trx.send_tx(id(), None).await
  }

  /// Places a long and short at the oracle price to capitalize on maker fees
  async fn build_orders(&self) -> anyhow::Result<Vec<OrderParams>> {
    let price = self
      .drift
      .market_info(self.market, &self.cache().await, None)?
      .price;
    let l3 = self
      .orderbook()
      .await
      .l3(&self.market, &self.cache().await)?;

    let quote_balance = self
      .drift
      .quote_balance(self.market, &self.cache().await, None)?;
    let trade_alloc_ratio = 50.0 / 100.0;
    let base_amt_f64 = quote_balance * self.leverage / price * trade_alloc_ratio;

    let real_quote_spread = l3.spread;
    let max_pct_spread = self.pct_max_spread;
    let max_quote_spread = max_pct_spread / 100.0 * price;
    let quote_spread = real_quote_spread.min(max_quote_spread) * self.pct_spread_multiplier / 100.0;
    let pct_spread = quote_spread / price * 100.0;
    info!(
      "spread: {}%, ${}",
      trunc!(pct_spread, 4),
      trunc!(quote_spread, 4),
    );

    let long_price = price - quote_spread / 2.0;
    let short_price = price + quote_spread / 2.0;

    let long = OrderParams {
      order_type: OrderType::Limit,
      market_type: self.market.kind,
      direction: PositionDirection::Long,
      user_order_id: 0,
      base_asset_amount: DriftUtils::base_to_u64(base_amt_f64),
      price: DriftUtils::price_to_u64(long_price),
      market_index: self.market.index,
      reduce_only: false,
      post_only: PostOnlyParam::MustPostOnly,
      immediate_or_cancel: false,
      max_ts: None,
      trigger_price: None,
      trigger_condition: OrderTriggerCondition::Above,
      oracle_price_offset: None,
      auction_duration: None,
      auction_start_price: None,
      auction_end_price: None,
    };
    let short = OrderParams {
      order_type: OrderType::Limit,
      market_type: self.market.kind,
      direction: PositionDirection::Short,
      user_order_id: 0,
      base_asset_amount: DriftUtils::base_to_u64(base_amt_f64),
      price: DriftUtils::price_to_u64(short_price),
      market_index: self.market.index,
      reduce_only: false,
      post_only: PostOnlyParam::MustPostOnly,
      immediate_or_cancel: false,
      max_ts: None,
      trigger_price: None,
      trigger_condition: OrderTriggerCondition::Below,
      oracle_price_offset: None,
      auction_duration: None,
      auction_start_price: None,
      auction_end_price: None,
    };
    Ok(vec![long, short])
  }

  /// Stream these accounts from geyser for usage in the engine
  pub async fn account_filter(
    &self,
    users: Vec<DecodedAcctCtx<User>>,
  ) -> anyhow::Result<Vec<Pubkey>> {
    let now = Instant::now();
    // accounts to subscribe to
    let perps = DriftUtils::perp_markets(&self.rpc()).await?;
    let spots = DriftUtils::spot_markets(&self.rpc()).await?;
    let perp_markets: Vec<Pubkey> = perps.iter().map(|p| p.key).collect();
    let spot_markets: Vec<Pubkey> = spots.iter().map(|s| s.key).collect();
    let user_keys: Vec<Pubkey> = users.iter().map(|u| u.key).collect();
    let perp_oracles: Vec<Pubkey> = perps.iter().map(|p| p.decoded.amm.oracle).collect();
    let spot_oracles: Vec<Pubkey> = spots.iter().map(|s| s.decoded.oracle).collect();
    let auths = [self.signer.pubkey()];
    self
      .cache
      .write()
      .await
      .load_with_all_users(&self.rpc(), Some(users), None, &auths)
      .await?;
    info!("cache loaded in {:?}", now.elapsed());
    let keys = perp_markets
      .iter()
      .chain(spot_markets.iter())
      .chain(user_keys.iter())
      .chain(perp_oracles.iter())
      .chain(spot_oracles.iter())
      .cloned()
      .collect::<Vec<Pubkey>>();
    Ok(keys)
  }

  pub fn new_tx(&self) -> KeypairTrx<'_> {
    self.drift.new_tx(true)
  }

  pub async fn place_orders(
    &self,
    orders: Vec<OrderParams>,
    trx: &mut KeypairTrx<'_>,
  ) -> anyhow::Result<()> {
    self
      .drift
      .place_orders_ix(&self.cache().await, orders, trx)
      .await?;
    Ok(())
  }

  pub async fn cancel_orders(
    &self,
    market: Option<MarketId>,
    direction: Option<PositionDirection>,
    trx: &mut KeypairTrx<'_>,
  ) -> anyhow::Result<()> {
    self
      .drift
      .cancel_orders_ix(&self.cache().await, market, direction, trx)
      .await?;
    Ok(())
  }

  pub async fn cancel_orders_by_ids(
    &self,
    orders: Vec<&Order>,
    trx: &mut KeypairTrx<'_>,
  ) -> anyhow::Result<()> {
    self
      .drift
      .cancel_orders_by_ids_ix(&self.cache().await, orders, trx)
      .await?;
    Ok(())
  }

  pub async fn close_positions(
    &self,
    markets: &[MarketId],
    trx: &mut KeypairTrx<'_>,
  ) -> anyhow::Result<()> {
    self
      .drift
      .close_perp_positions(&self.cache().await, markets, false, false, trx)
      .await
  }

  pub async fn arb_perp(
    &self,
    makers: Vec<MakerInfo>,
    trx: &mut KeypairTrx<'_>,
  ) -> anyhow::Result<()> {
    self
      .drift
      .arb_perp_ix(&self.cache().await, self.market, makers, trx)
      .await
  }
}
