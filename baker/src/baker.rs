#![allow(unused_imports)]

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Deref;
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
use yellowstone_grpc_proto::prelude::subscribe_request_filter_accounts_filter::Filter;
use yellowstone_grpc_proto::prelude::{
  subscribe_request_filter_accounts_filter_memcmp, CommitmentLevel, SubscribeRequestFilterAccounts,
  SubscribeRequestFilterAccountsFilter, SubscribeRequestFilterAccountsFilterMemcmp,
  SubscribeRequestFilterBlocks, SubscribeRequestFilterBlocksMeta, SubscribeRequestFilterSlots,
  SubscribeRequestFilterTransactions,
};

use crate::config::BakerConfig;
use nexus::drift_client::*;
use nexus::drift_cpi::{Decode, DiscrimToName, InstructionType, MarketType};
use nexus::*;

pub struct Baker {
  read_only: bool,
  retry_until_confirmed: bool,
  pub signer: Arc<Keypair>,
  pub rpc: Arc<RpcClient>,
  pub drift: DriftClient,
  pub market: MarketId,
  pub cache: Cache,
  pub pct_cancel_threshold: f64,
}

impl Baker {
  pub async fn new(
    sub_account_id: u16,
    market: MarketId,
    cache_depth: Option<usize>,
  ) -> anyhow::Result<Self> {
    let BakerConfig {
      read_only,
      retry_until_confirmed,
      signer,
      rpc_url,
      grpc,
      x_token,
      pct_cancel_threshold,
      ..
    } = BakerConfig::read()?;

    // 200 slots = 80 seconds of account cache
    let cache_depth = cache_depth.unwrap_or(200);
    let signer = Arc::new(signer);
    info!("Baker using wallet: {}", signer.pubkey());
    let rpc = Arc::new(RpcClient::new_with_timeout(
      rpc_url,
      Duration::from_secs(90),
    ));

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
      market,
      pct_cancel_threshold,
    };

    let account_filter = this.account_filter().await?;
    let cfg = this.geyser_config(grpc, x_token, account_filter)?;
    // stream updates from gRPC
    let nexus = NexusClient::new(cfg)?;
    let cache = this.cache.clone();
    tokio::task::spawn(async move {
      nexus.stream(&cache, None, None, None).await?;
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
        // subscribe to all Drift `User` accounts
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

      let mut trx = self.new_tx();
      match (long_order, short_order) {
        (Some(long), Some(short)) => {
          let market_info = self
            .drift
            .market_info(self.market, &self.cache().await, None)?;
          let long_price = long.price as f64 / PRICE_PRECISION as f64;
          let long_price_pct_diff = (market_info.price - long_price) / long_price * 100.0;
          let short_price = short.price as f64 / PRICE_PRECISION as f64;
          let short_price_pct_diff = (market_info.price - short_price) / short_price * 100.0;

          let long_filled = matches!(long.status, OrderStatus::Filled);
          let short_filled = matches!(short.status, OrderStatus::Filled);

          if long_filled && short_filled {
            info!("both filled, place orders");
            let params = self.build_orders().await?;
            self.place_orders(params, &mut trx).await?;
          } else if short_price_pct_diff.abs() > self.pct_cancel_threshold
            || long_price_pct_diff.abs() > self.pct_cancel_threshold
          {
            warn!("price moved {}%, cancel orders", self.pct_cancel_threshold);
            // Does not matter if either order is filled, as price has moved too much to ensure both are filled.
            self
              .cancel_orders_by_ids(vec![&long, &short], &mut trx)
              .await?;
            self.close_perp_positions(&[self.market], &mut trx).await?;
          }
        }
        (None, None) => {
          info!("none, place orders");
          let params = self.build_orders().await?;
          self.place_orders(params, &mut trx).await?;
        }
        _ => {}
      }

      trx.send_tx(id(), None).await?;

      tokio::time::sleep(Duration::from_millis(200)).await;
    }

    Ok(())
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
    self.close_perp_positions(&[self.market], &mut trx).await?;
    trx.send_tx(id(), None).await
  }

  /// Places a long and short at the oracle price to capitalize on maker fees
  async fn build_orders(&self) -> anyhow::Result<Vec<OrderParams>> {
    let cache = self.cache().await;
    let market_info = self.drift.market_info(self.market, &cache, None)?;
    let quote_balance = self.drift.quote_balance(self.market, &cache, None)?;
    let quote_balance = quote_balance * 0.98;
    let trade_alloc_ratio = 50.0 / 100.0;
    let base_amt_f64 = quote_balance / market_info.price * trade_alloc_ratio;
    let base_asset_amount = (base_amt_f64 * BASE_PRECISION as f64).round() as u64;
    let price = (market_info.price * PRICE_PRECISION as f64).round() as u64;
    let long = OrderParams {
      order_type: OrderType::Limit,
      market_type: self.market.kind,
      direction: PositionDirection::Long,
      user_order_id: 0,
      base_asset_amount,
      price,
      market_index: self.market.index,
      reduce_only: false,
      post_only: PostOnlyParam::MustPostOnly,
      immediate_or_cancel: false,
      max_ts: None,
      trigger_price: None,
      trigger_condition: Default::default(),
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
      base_asset_amount,
      price,
      market_index: self.market.index,
      reduce_only: false,
      post_only: PostOnlyParam::MustPostOnly,
      immediate_or_cancel: false,
      max_ts: None,
      trigger_price: None,
      trigger_condition: Default::default(),
      oracle_price_offset: None,
      auction_duration: None,
      auction_start_price: None,
      auction_end_price: None,
    };
    Ok(vec![long, short])
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

  pub fn new_tx(&self) -> TrxBuilder<'_, Keypair, Vec<&Keypair>> {
    self.drift.new_tx(true)
  }

  pub async fn place_and_take_order(
    &self,
    order: OrderParams,
    maker_info: Option<User>,
    fulfillment_type: Option<SpotFulfillmentType>,
    trx: &mut TrxBuilder<'_, Keypair, Vec<&Keypair>>,
  ) -> anyhow::Result<()> {
    self
      .drift
      .place_and_take_order_ix(
        &self.cache().await,
        order,
        maker_info,
        fulfillment_type,
        trx,
      )
      .await
  }

  pub async fn place_orders(
    &self,
    orders: Vec<OrderParams>,
    trx: &mut TrxBuilder<'_, Keypair, Vec<&Keypair>>,
  ) -> anyhow::Result<()> {
    self
      .drift
      .place_orders_ix(&self.cache().await, orders, trx)
      .await
  }

  pub async fn cancel_orders(
    &self,
    market: Option<MarketId>,
    direction: Option<PositionDirection>,
    trx: &mut TrxBuilder<'_, Keypair, Vec<&Keypair>>,
  ) -> anyhow::Result<()> {
    info!("cancel orders...");
    self
      .drift
      .cancel_orders_ix(&self.cache().await, market, direction, trx)
      .await
  }

  pub async fn cancel_orders_by_ids(
    &self,
    orders: Vec<&Order>,
    trx: &mut TrxBuilder<'_, Keypair, Vec<&Keypair>>,
  ) -> anyhow::Result<()> {
    info!("cancel orders by ids...");
    self
      .drift
      .cancel_orders_by_ids_ix(&self.cache().await, orders, trx)
      .await
  }

  pub async fn close_perp_positions(
    &self,
    markets: &[MarketId],
    trx: &mut TrxBuilder<'_, Keypair, Vec<&Keypair>>,
  ) -> anyhow::Result<()> {
    info!("close positions...");
    self
      .drift
      .close_perp_positions(&self.cache().await, markets, trx)
      .await
  }
}
