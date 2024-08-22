#![allow(dead_code)]

use std::collections::HashSet;
use std::ops::Neg;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use log::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use tokio::time::Instant;
use tradestats::metrics::spread_dynamic;
use yellowstone_grpc_proto::prelude::{
  CommitmentLevel, SubscribeRequestFilterAccounts, SubscribeRequestFilterSlots,
};

use crate::config::Config;
use nexus::drift_client::*;
use nexus::*;
use nexus::{Data, Dataset};

pub struct Engine {
  read_only: bool,
  retry_until_confirmed: bool,
  pub signer: Arc<Keypair>,
  pub rpc: Arc<RpcClient>,
  pub drift: DriftClient,
  pub market: MarketId,
  pub cache: Cache,
  pub orderbook: Orderbook,
  pct_stop_loss: f64,
  leverage: f64,
  stop_loss_is_maker: bool,
  zscore_threshold: f64,
  zscore_window: usize,
  cache_depth: usize,
}

impl Engine {
  pub async fn new(sub_account_id: u16, market: MarketId) -> anyhow::Result<Self> {
    let Config {
      read_only,
      retry_until_confirmed,
      signer,
      rpc_url,
      grpc,
      x_token,
      pct_stop_loss,
      leverage,
      stop_loss_is_maker,
      zscore_threshold,
      zscore_window,
      cache_depth,
      ..
    } = Config::read()?;

    let signer = Arc::new(signer);
    info!("Engine using wallet: {}", signer.pubkey());
    let rpc = Arc::new(RpcClient::new_with_timeout(
      rpc_url,
      Duration::from_secs(90),
    ));
    let now = Instant::now();
    let orderbook = Orderbook::new_from_rpc(vec![market], &rpc).await?;
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
      pct_stop_loss,
      leverage,
      stop_loss_is_maker,
      zscore_threshold,
      zscore_window,
      cache_depth,
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

          if short_price_pct_diff > self.pct_stop_loss
            || long_price_pct_diff < self.pct_stop_loss * -1.0
          {
            info!(
              "🔴 price moved {}% beyond spread orders, reset position",
              self.pct_stop_loss
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

          let pct_diff = market_info.price / short_price * 100.0 - 100.0;
          // if price moves far above the ask, the bid likely won't fill
          if pct_diff > self.pct_stop_loss {
            info!(
              "🔴 price moved {}% above short entry, reset position",
              self.pct_stop_loss
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
          debug!("long pos: ${}", trunc!(long_price, 3));

          let pct_diff = market_info.price / long_price * 100.0 - 100.0;
          // if price moves far below the bid, the ask likely won't fill
          if pct_diff < self.pct_stop_loss.neg() {
            info!(
              "🔴 price moved -{}% below long entry, reset position",
              self.pct_stop_loss
            );
            self.reset(false).await?;
            did_act = true;
          }
        }
        (None, None, None, None) => {
          let orders = self.build_orders().await?;
          if !orders.is_empty() {
            info!("🟢 place orders");
            // no open orders or positions, place new orders
            let mut trx = self.new_tx();
            self
              .place_orders(self.build_orders().await?, &mut trx)
              .await?;
            trx.send_tx(id(), None).await?;
            did_act = true;
          }
        }
        _ => {}
      }

      if did_act {
        last_update = Instant::now();
      }
      if last_update.elapsed() > Duration::from_secs(60 * 10) && !self.read_only {
        info!("🔴 no activity for 10 minutes, reset position");
        self.reset(true).await?;
      }
      tokio::time::sleep(Duration::from_millis(400)).await;
    }

    Ok(())
  }

  /// ZScore of last index in a spread time series
  fn zscore(&self, series: &[f64]) -> anyhow::Result<f64> {
    // Guard: Ensure correct window size
    if self.zscore_window > series.len() {
      return Err(anyhow::anyhow!("Window size is greater than vector length"));
    }

    // last z score
    let window_data: &[f64] = &series[series.len() - self.zscore_window..];
    let mean: f64 = window_data.iter().sum::<f64>() / window_data.len() as f64;
    let var: f64 = window_data
      .iter()
      .map(|&val| (val - mean).powi(2))
      .sum::<f64>()
      / (window_data.len() - 1) as f64;
    let std_dev: f64 = var.sqrt();
    if std_dev == 0.0 {
      return Err(anyhow::anyhow!("Standard deviation is zero"));
    }
    let z_score = (series[series.len() - 1] - mean) / std_dev;
    Ok(z_score)
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
    let cache = self.cache().await;
    let price = self.drift.market_info(self.market, &cache, None)?.price;

    let quote_balance = self.drift.quote_balance(self.market, &cache, None)?;
    let trade_alloc_ratio = 50.0 / 100.0;
    let base_amt = quote_balance * self.leverage / price * trade_alloc_ratio;

    let sol_ticker = MarketId::SOL_PERP;
    let btc_ticker = MarketId::perp(1);
    let x_series = Dataset::new(
      cache
        .ring(&sol_ticker.key())?
        .key_values()
        .flat_map(|(k, v)| {
          let price = self.drift.market_info(sol_ticker, &cache, Some(v.slot))?;
          Result::<_, anyhow::Error>::Ok(Data {
            x: *k as i64,
            y: price.price,
          })
        })
        .collect(),
    );
    let y_series = Dataset::new(
      cache
        .ring(&btc_ticker.key())?
        .key_values()
        .flat_map(|(k, v)| {
          let price = self.drift.market_info(btc_ticker, &cache, Some(v.slot))?;
          Result::<_, anyhow::Error>::Ok(Data {
            x: *k as i64,
            y: price.price,
          })
        })
        .collect(),
    );

    if x_series.len() < self.zscore_window || y_series.len() < self.zscore_window {
      warn!(
        "Insufficient data for zscore calculation, x: {}, y: {}",
        x_series.len(),
        y_series.len()
      );
      return Ok(vec![]);
    }

    let windows = Dataset::new(
      x_series
        .data()
        .windows(2)
        .map(|x| Data {
          x: x[1].x,
          y: x[1].y - x[0].y,
        })
        .collect(),
    );
    let mut y_series = Dataset::new(windows.data()[..windows.data().len() - 1].to_vec());
    let mut x_series = Dataset::new(windows.data()[1..windows.len()].to_vec());

    Dataset::align(&mut x_series, &mut y_series)?;

    let latest_x = x_series
      .data()
      .last()
      .ok_or(anyhow::anyhow!("No X ticker data"))?;
    let _latest_y = y_series
      .data()
      .last()
      .ok_or(anyhow::anyhow!("No Y ticker data"))?;

    // let x = x_series.normalize_series()?;
    // let y = y_series.normalize_series()?;
    // let spread = match spread_standard(&x.y(), &y.y()) {
    //   Err(e) => {
    //     if e.to_string().contains("The variance of x values is zero") {
    //       return Ok(vec![]);
    //     } else {
    //       return Err(anyhow::anyhow!("Error calculating spread: {}", e));
    //     }
    //   }
    //   Ok(res) => res,
    // };
    // let lag_spread = spread[..spread.len() - 1].to_vec();
    // let spread = spread[1..].to_vec();
    //
    // assert_eq!(spread.len(), lag_spread.len());
    // assert_eq!(lag_spread.len(), self.zscore_window);
    // assert_eq!(spread.len(), self.zscore_window);

    // let x = x_series.normalize_series()?;
    // let y = y_series.normalize_series()?;
    let x = x_series.clone();
    let y = y_series.clone();
    let spread = match spread_dynamic(&x.y(), &y.y()) {
      Err(e) => {
        if e.to_string().contains("The variance of x values is zero") {
          warn!("The variance of x values is zero");
          return Ok(vec![]);
        } else {
          return Err(anyhow::anyhow!("Error calculating spread: {}", e));
        }
      }
      Ok(res) => res,
    };

    let z_0 = Data {
      x: latest_x.x,
      y: self.zscore(&spread)?,
    };
    info!("y: {}, zscore: {}", trunc!(latest_x.y, 3), z_0.y);

    let short = z_0.y < -self.zscore_threshold;
    let long = z_0.y > self.zscore_threshold;

    let mut orders = vec![];

    if long {
      let long_order = OrderParams {
        order_type: OrderType::Limit,
        market_type: self.market.kind,
        direction: PositionDirection::Long,
        user_order_id: 0,
        base_asset_amount: DriftUtils::base_to_u64(base_amt),
        price: DriftUtils::price_to_u64(price),
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
      orders.push(long_order);
    } else if short {
      let short_order = OrderParams {
        order_type: OrderType::Limit,
        market_type: self.market.kind,
        direction: PositionDirection::Short,
        user_order_id: 0,
        base_asset_amount: DriftUtils::base_to_u64(base_amt),
        price: DriftUtils::price_to_u64(price),
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
      orders.push(short_order)
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
      .close_perp_positions(
        &self.cache().await,
        markets,
        self.stop_loss_is_maker,
        false,
        trx,
      )
      .await
  }
}
