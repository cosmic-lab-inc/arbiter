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

use crate::config::BracketeerConfig;
use nexus::drift_client::*;
use nexus::drift_cpi::{Decode, DiscrimToName, InstructionType, MarketType};
use nexus::*;

struct OrderStub {
  pub price: f64,
  pub base: f64,
}

pub struct Bracketeer {
  read_only: bool,
  retry_until_confirmed: bool,
  pub signer: Arc<Keypair>,
  pub rpc: Arc<RpcClient>,
  pub drift: DriftClient,
  pub market: MarketId,
  pub cache: Cache,
  pub orderbook: Orderbook,
  pct_spread_brackets: Vec<f64>,
  pct_exit_deviation: f64,
  leverage: f64,
  pct_max_spread: f64,
  pct_min_spread: f64,
}

impl Bracketeer {
  pub async fn new(
    sub_account_id: u16,
    market: MarketId,
    cache_depth: Option<usize>,
  ) -> anyhow::Result<Self> {
    let BracketeerConfig {
      read_only,
      retry_until_confirmed,
      signer,
      rpc_url,
      grpc,
      x_token,
      pct_spread_brackets,
      pct_exit_deviation,
      leverage,
      pct_max_spread,
      pct_min_spread,
      ..
    } = BracketeerConfig::read()?;

    // 200 slots = 80 seconds of account cache
    let cache_depth = cache_depth.unwrap_or(200);
    let signer = Arc::new(signer);
    info!("Bracketeer using wallet: {}", signer.pubkey());
    let rpc = Arc::new(RpcClient::new_with_timeout(
      rpc_url,
      Duration::from_secs(90),
    ));
    let orderbook = Orderbook::new_from_rpc(vec![market], &rpc).await?;

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
      pct_spread_brackets,
      pct_exit_deviation,
      leverage,
      pct_max_spread,
      pct_min_spread,
    };

    let account_filter = this.account_filter().await?;
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
    self.reset(true).await?;
    let run = AtomicBool::new(true);

    let mut last_update = Instant::now();
    while run.load(Ordering::Relaxed) {
      let mut did_act = false;
      let user = self
        .cache()
        .await
        .decoded_account::<User>(self.user(), None)?
        .decoded;

      let long_orders: Vec<&Order> = user
        .orders
        .iter()
        .filter(|o| {
          MarketId::from((o.market_index, o.market_type)) == self.market
            && matches!(o.direction, PositionDirection::Long)
            && o.base_asset_amount != 0
        })
        .collect();

      let short_orders: Vec<&Order> = user
        .orders
        .iter()
        .filter(|o| {
          MarketId::from((o.market_index, o.market_type)) == self.market
            && matches!(o.direction, PositionDirection::Short)
            && o.base_asset_amount != 0
        })
        .collect();

      let long_order = match long_orders.is_empty() {
        true => None,
        false => Some(OrderStub {
          price: long_orders
            .iter()
            .map(|o| DriftUtils::price_to_f64(o.price))
            .sum::<f64>()
            / long_orders.len() as f64,
          base: long_orders
            .iter()
            .map(|o| DriftUtils::base_to_f64(o.base_asset_amount))
            .sum::<f64>()
            / long_orders.len() as f64,
        }),
      };
      let short_order = match short_orders.is_empty() {
        true => None,
        false => Some(OrderStub {
          price: short_orders
            .iter()
            .map(|o| DriftUtils::price_to_f64(o.price))
            .sum::<f64>()
            / short_orders.len() as f64,
          base: short_orders
            .iter()
            .map(|o| DriftUtils::base_to_f64(o.base_asset_amount))
            .sum::<f64>()
            / short_orders.len() as f64,
        }),
      };

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
          let long_price_pct_diff = market_info.price / lo.price * 100.0 - 100.0;
          let short_price_pct_diff = market_info.price / so.price * 100.0 - 100.0;

          if short_price_pct_diff > self.pct_exit_deviation
            || long_price_pct_diff < self.pct_exit_deviation.neg()
          {
            info!(
              "🔴 price moved {}% beyond spread orders, reset position",
              self.pct_exit_deviation
            );
            self.reset(false).await?;
            did_act = true;
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
          if pct_diff > self.pct_exit_deviation {
            info!(
              "🔴 price moved {}% above short entry, reset position",
              self.pct_exit_deviation
            );
            self.reset(false).await?;
            did_act = true;
          }
        }
        (None, Some(_), Some(lpos), None) => {
          let market_info = self
            .drift
            .market_info(self.market, &self.cache().await, None)?;

          let long_price = DriftUtils::perp_position_price(lpos);

          let pct_diff = market_info.price / long_price * 100.0 - 100.0;
          // if price moves far below the bid, the ask likely won't fill
          if pct_diff < self.pct_exit_deviation.neg() {
            info!(
              "🔴 price moved -{}% below long entry, reset position",
              self.pct_exit_deviation
            );
            self.reset(false).await?;
            did_act = true;
          }
        }
        (None, None, None, None) => {
          info!("🟢 place orders");
          // no open orders or positions, place new orders
          let mut trx = self.new_tx();
          self
            .place_orders(self.build_orders().await?, &mut trx)
            .await?;
          trx.send_tx(id(), None).await?;
          did_act = true;
        }
        _ => {}
      }

      if did_act {
        last_update = Instant::now();
      }
      if last_update.elapsed() > Duration::from_secs(60 * 2) {
        info!("🔴 no activity for 2 minutes, reset position");
        self.reset(true).await?;
      }
      tokio::time::sleep(Duration::from_millis(200)).await;
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

  async fn reset(&self, retry: bool) -> anyhow::Result<()> {
    let mut trx = self.new_tx();
    trx.retry_until_confirmed = retry;
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
    let total_quote = quote_balance * self.leverage;

    let num_orders = self.pct_spread_brackets.len() * 2;
    let min_base = total_quote / price / num_orders as f64;

    let max_quote_spread = self.pct_max_spread / 100.0 * price;
    let min_quote_spread = self.pct_min_spread / 100.0 * price;
    let quote_spread = min_quote_spread.max(l3.spread.min(max_quote_spread));
    let pct_spread = quote_spread / price * 100.0;

    let mut orders = vec![];
    for pct_spread_bracket in self.pct_spread_brackets.iter() {
      let bid_base = DriftUtils::base_to_u64(min_base);
      let ask_base = DriftUtils::base_to_u64(min_base);

      let bracket_mul = pct_spread_bracket / 100.0;
      info!(
        "spread: {}%, ${}",
        trunc!(pct_spread * bracket_mul, 4),
        trunc!(quote_spread * bracket_mul, 4),
      );

      // lowest pnl
      let bid_price = price - quote_spread * bracket_mul / 2.0;
      let ask_price = price + quote_spread * bracket_mul / 2.0;
      assert!(bid_price < price && ask_price > price);

      let bid = OrderParams {
        order_type: OrderType::Limit,
        market_type: self.market.kind,
        direction: PositionDirection::Long,
        user_order_id: 0,
        base_asset_amount: bid_base,
        price: DriftUtils::price_to_u64(bid_price),
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
      let ask = OrderParams {
        order_type: OrderType::Limit,
        market_type: self.market.kind,
        direction: PositionDirection::Short,
        user_order_id: 0,
        base_asset_amount: ask_base,
        price: DriftUtils::price_to_u64(ask_price),
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
      orders.push(bid);
      orders.push(ask);
    }
    Ok(orders)
  }

  /// Stream these accounts from geyser for usage in the engine
  pub async fn account_filter(&self) -> anyhow::Result<Vec<Pubkey>> {
    // accounts to subscribe to
    let perps = DriftUtils::perp_markets(&self.rpc()).await?;
    let spots = DriftUtils::spot_markets(&self.rpc()).await?;
    let perp_markets: Vec<Pubkey> = perps.iter().map(|p| p.key).collect();
    let spot_markets: Vec<Pubkey> = spots.iter().map(|s| s.key).collect();
    let user = DriftUtils::user_pda(&self.signer.pubkey(), 0);
    let users = [user];
    let perp_oracles: Vec<Pubkey> = perps.iter().map(|p| p.decoded.amm.oracle).collect();
    let spot_oracles: Vec<Pubkey> = spots.iter().map(|s| s.decoded.oracle).collect();
    let auths = [self.signer.pubkey()];
    self
      .cache
      .write()
      .await
      .load(&self.rpc(), &users, None, &auths)
      .await?;
    let keys = perp_markets
      .iter()
      .chain(spot_markets.iter())
      .chain(users.iter())
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
      .close_perp_positions(&self.cache().await, markets, trx)
      .await
  }
}
