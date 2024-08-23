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
use yellowstone_grpc_proto::prelude::{
  CommitmentLevel, SubscribeRequestFilterAccounts, SubscribeRequestFilterSlots,
};

use crate::config::Config;
use nexus::drift_client::*;
use nexus::*;

const TAKE_PROFIT_ID: u8 = 111;

struct OrderStub {
  pub price: f64,
  pub base: f64,
}

pub struct Engine {
  read_only: bool,
  retry_until_confirmed: bool,
  pub signer: Arc<Keypair>,
  pub rpc: Arc<RpcClient>,
  pub drift: DriftClient,
  pub market: MarketId,
  pub cache: Cache,
  pub orderbook: Orderbook,
  pct_spread_brackets: Vec<f64>,
  pct_stop_loss: f64,
  leverage: f64,
  pct_max_spread: f64,
  pct_min_spread: f64,
  stop_loss_is_maker: bool,
  pct_take_profit: f64,
}

impl Engine {
  pub async fn new(
    sub_account_id: u16,
    market: MarketId,
    cache_depth: Option<usize>,
  ) -> anyhow::Result<Self> {
    let Config {
      read_only,
      retry_until_confirmed,
      signer,
      rpc_url,
      grpc,
      x_token,
      pct_spread_brackets,
      pct_stop_loss,
      leverage,
      pct_max_spread,
      pct_min_spread,
      stop_loss_is_maker,
      pct_take_profit,
      ..
    } = Config::read()?;

    // 200 slots = 80 seconds of account cache
    let cache_depth = cache_depth.unwrap_or(200);
    let signer = Arc::new(signer);
    info!("Engine using wallet: {}", signer.pubkey());
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
      pct_spread_brackets,
      pct_stop_loss,
      leverage,
      pct_max_spread,
      pct_min_spread,
      stop_loss_is_maker,
      pct_take_profit,
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
            && o.user_order_id != TAKE_PROFIT_ID
        })
        .collect();

      let short_orders: Vec<&Order> = user
        .orders
        .iter()
        .filter(|o| {
          MarketId::from((o.market_index, o.market_type)) == self.market
            && matches!(o.direction, PositionDirection::Short)
            && o.base_asset_amount != 0
            && o.user_order_id != TAKE_PROFIT_ID
        })
        .collect();

      let take_profit_order = user
        .orders
        .iter()
        .find(|o| o.user_order_id == TAKE_PROFIT_ID);

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
        // bid ask orders still open
        (Some(lo), Some(so), None, None) => {
          let price = self
            .drift
            .market_info(self.market, &self.cache().await, None)?
            .price;
          let long_price_pct_diff = price / lo.price * 100.0 - 100.0;
          let short_price_pct_diff = price / so.price * 100.0 - 100.0;

          if short_price_pct_diff > self.pct_stop_loss
            || long_price_pct_diff < self.pct_stop_loss.neg()
          {
            info!(
              "🔴 price moved {}% beyond spread orders, reset position",
              self.pct_stop_loss
            );
            self.reset(false).await?;
            did_act = true;
          }
        }
        // bid order open, ask order filled (short position)
        (Some(_), None, None, Some(spos)) => {
          // check stop loss:
          // if price moves far above the ask, the bid likely won't fill
          let price = self
            .drift
            .market_info(self.market, &self.cache().await, None)?
            .price;
          let short_price = DriftUtils::perp_position_price(spos);
          let pct_diff = price / short_price * 100.0 - 100.0;
          if pct_diff > self.pct_stop_loss {
            info!(
              "🔴 price moved {}% above short entry, reset position",
              self.pct_stop_loss
            );
            self.reset(false).await?;
            did_act = true;
          }

          // check take profit:
          // search for maker asks at a profitable level to "take" against to close this short for a profit
          let l3 = self
            .orderbook()
            .await
            .l3(&self.market, &self.cache().await)?;
          // taker fee is 0.025%, profit threshold beyond that is defined in the config
          let pct_cutoff = 0.025 + self.pct_take_profit;
          let take_profit_ask =
            l3.take_profit_maker_asks(spos, pct_cutoff, self.cache().await.slot)?;
          if let Some(take_profit_ask) = take_profit_ask {
            let new_tp_order = self.build_take_profit_order(spos, take_profit_ask).await?;
            let maker_user = self
              .cache()
              .await
              .decoded_account::<User>(&take_profit_ask.user, None)?
              .decoded;

            if let Some(existing_tp_order) = take_profit_order {
              if existing_tp_order.price == new_tp_order.price
                && existing_tp_order.base_asset_amount == new_tp_order.base_asset_amount
              {
                warn!("take profit order already exists");
                continue;
              }
            }

            info!("🟢 take profit on short");
            let mut trx = self.new_tx();
            self
              .place_and_take_order(new_tp_order, Some(maker_user), &mut trx)
              .await?;
            trx.send_tx(id(), None).await?;
            did_act = true;
          }
        }
        // ask order open, bid order filled (long position)
        (None, Some(_), Some(lpos), None) => {
          // if price moves far below the bid, the ask likely won't fill
          let price = self
            .drift
            .market_info(self.market, &self.cache().await, None)?
            .price;
          let long_price = DriftUtils::perp_position_price(lpos);
          let pct_diff = price / long_price * 100.0 - 100.0;
          if pct_diff < self.pct_stop_loss.neg() {
            info!(
              "🔴 price moved -{}% below long entry, reset position",
              self.pct_stop_loss
            );
            self.reset(false).await?;
            did_act = true;
          }

          // check take profit:
          // search for maker bids at a profitable level to "take" against to close this long for a profit
          let l3 = self
            .orderbook()
            .await
            .l3(&self.market, &self.cache().await)?;
          // taker fee is 0.025%, profit beyond that is defined in the config
          let pct_cutoff = 0.025 + self.pct_take_profit;
          let take_profit_bid =
            l3.take_profit_maker_bids(lpos, pct_cutoff, self.cache().await.slot)?;
          if let Some(take_profit_bid) = take_profit_bid {
            let new_tp_order = self.build_take_profit_order(lpos, take_profit_bid).await?;
            let maker_user = self
              .cache()
              .await
              .decoded_account::<User>(&take_profit_bid.user, None)?
              .decoded;

            if let Some(existing_tp_order) = take_profit_order {
              if existing_tp_order.price == new_tp_order.price
                && existing_tp_order.base_asset_amount == new_tp_order.base_asset_amount
              {
                warn!("take profit order already exists");
                continue;
              }
            }

            info!("🟢 take profit on long");
            let mut trx = self.new_tx();
            self
              .place_and_take_order(new_tp_order, Some(maker_user), &mut trx)
              .await?;
            trx.send_tx(id(), None).await?;
            did_act = true;
          }
        }
        // no orders or positions, start new trade
        (None, None, None, None) => {
          info!("🟢 place orders");
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
      if last_update.elapsed() > Duration::from_secs(60 * 10) {
        info!("🔴 no activity for 5 minutes, reset position");
        self.reset(true).await?;
        last_update = Instant::now();
      }
      tokio::time::sleep(Duration::from_millis(400)).await;
    }

    Ok(())
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

  async fn build_take_profit_order(
    &self,
    pos: &PerpPosition,
    maker: &OrderInfo,
  ) -> anyhow::Result<OrderParams> {
    // take profit order is opposite position direction
    let tp_dir = match pos.base_asset_amount > 0 {
      true => PositionDirection::Short,
      false => PositionDirection::Long,
    };
    let base_asset_amount = pos
      .base_asset_amount
      .unsigned_abs()
      .min(maker.order.base_asset_amount);
    let tp_price = DriftUtils::price_to_u64(maker.price);
    let trigger_condition = match pos.base_asset_amount > 0 {
      // position is long so take profit is short, which means trigger below price
      true => OrderTriggerCondition::Below,
      // position is short so take profit is long, which means trigger above price
      false => OrderTriggerCondition::Above,
    };
    let order = OrderParams {
      order_type: OrderType::Limit,
      market_type: self.market.kind,
      direction: tp_dir,
      user_order_id: 0,
      base_asset_amount,
      price: tp_price,
      market_index: self.market.index,
      reduce_only: false,
      post_only: PostOnlyParam::None,
      immediate_or_cancel: true,
      max_ts: Some(Time::now().to_unix() + 15),
      trigger_price: None,
      trigger_condition,
      oracle_price_offset: None,
      auction_duration: None,
      auction_start_price: None,
      auction_end_price: None,
    };
    Ok(order)
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
  pub async fn account_filter(
    &self,
    users: Vec<DecodedAcctCtx<User>>,
  ) -> anyhow::Result<Vec<Pubkey>> {
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
      .close_perp_positions(
        &self.cache().await,
        markets,
        self.stop_loss_is_maker,
        false,
        trx,
      )
      .await
  }

  pub async fn place_and_take_order(
    &self,
    order: OrderParams,
    maker: Option<User>,
    trx: &mut KeypairTrx<'_>,
  ) -> anyhow::Result<()> {
    self
      .drift
      .place_and_take_order_ix(&self.cache().await, order, maker, None, trx)
      .await
  }

  pub async fn perp_take_profit(
    &self,
    pos: &PerpPosition,
    tp_price: f64,
    trx: &mut KeypairTrx<'_>,
  ) -> anyhow::Result<()> {
    self
      .drift
      .perp_take_profit(&self.cache().await, pos, tp_price, trx)
      .await
  }
}

#[test]
fn loan_leverage() {
  let ltv = 0.75; // $1000 in collateral can borrow $750
  let initial_collateral = 1000.0;
  let mut collateral = initial_collateral;
  let mut borrowed = 0.0;
  let mut interest = 0.0;
  let mut loops = 0;
  let borrow_rate = 6.68;

  fn loop_borrow(
    ltv: f64,
    borrow_rate: f64,
    collateral: &mut f64,
    borrowed: &mut f64,
    loops: &mut usize,
    interest: &mut f64,
  ) {
    let loan = *collateral * ltv;
    let rate = loan * borrow_rate / 100.0;
    let new_borrowed = *borrowed + loan + rate;
    let new_collateral = *collateral + loan;
    if new_borrowed > new_collateral * ltv {
      // max leverage reached, will be liquidated if looped again
      println!("Max leverage reached");
    } else {
      *borrowed += loan;
      *collateral += loan;
      *interest += rate;
      *loops += 1;
      println!("borrowed: {}", *borrowed);
      println!("collateral: {}", *collateral);
      println!("interest: {}", *interest);
      println!("loops: {}", *loops);
      loop_borrow(ltv, borrow_rate, collateral, borrowed, loops, interest);
    }
  }

  loop_borrow(
    ltv,
    borrow_rate,
    &mut collateral,
    &mut borrowed,
    &mut loops,
    &mut interest,
  );
  println!("Loops: {}", loops);
  println!("Borrowed: {}", borrowed);
  println!("Collateral: {}", collateral);
  println!("Leverage: {}", trunc!(collateral / initial_collateral, 2));
  println!(
    "Interest Paid: ${} or {}%",
    trunc!(interest, 2),
    trunc!(interest / borrowed * 100.0, 2)
  );
}
