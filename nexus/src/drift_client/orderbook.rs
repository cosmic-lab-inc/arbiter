use rayon::prelude::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use std::collections::HashMap;
use std::sync::Arc;

use drift_cpi::{Order, User};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::{Mutex, MutexGuard};

use crate::drift_client::{DlobNode, DriftUtils, L3Orderbook, MarketId, OrderInfo, ReadCache};
use crate::DecodedAcctCtx;

type MarketKey = Pubkey;
type UserKey = Pubkey;

pub type ReadOrderbook<'a> = MutexGuard<'a, InnerOrderbook>;
pub type WriteOrderbook<'a> = MutexGuard<'a, InnerOrderbook>;

#[derive(Default)]
pub struct Orderbook {
  orderbook: Arc<Mutex<InnerOrderbook>>,
}

impl Clone for Orderbook {
  fn clone(&self) -> Self {
    Self {
      orderbook: self.orderbook.clone(),
    }
  }
}

impl Orderbook {
  pub fn new(markets: Vec<MarketId>) -> Self {
    Self {
      orderbook: Arc::new(Mutex::new(InnerOrderbook::new(markets))),
    }
  }

  pub async fn read(&self) -> ReadOrderbook {
    self.orderbook.lock().await
  }

  pub async fn write(&self) -> WriteOrderbook {
    self.orderbook.lock().await
  }
}

#[derive(Default)]
pub struct InnerOrderbook {
  markets: Vec<MarketId>,
  orderbook: HashMap<MarketKey, HashMap<UserKey, Vec<DlobNode>>>,
}

impl InnerOrderbook {
  pub fn new(markets: Vec<MarketId>) -> Self {
    Self {
      markets,
      orderbook: HashMap::new(),
    }
  }

  pub fn ready(&self, market: &MarketKey) -> bool {
    !self
      .orderbook
      .get(market)
      .unwrap_or(&HashMap::new())
      .is_empty()
  }

  pub fn market_users(&self, market: &MarketId) -> anyhow::Result<usize> {
    let dlob = self.orders(&market.key())?;
    Ok(dlob.keys().collect::<Vec<&UserKey>>().len())
  }

  pub fn market_orders(&self, market: &MarketId) -> anyhow::Result<usize> {
    let dlob = self.orders(&market.key())?;
    Ok(
      dlob
        .values()
        .collect::<Vec<&Vec<DlobNode>>>()
        .par_iter()
        .map(|o| o.len())
        .sum::<usize>(),
    )
  }

  pub fn l3(&self, market: &MarketId, cache: &ReadCache<'_>) -> anyhow::Result<L3Orderbook> {
    let mut bids: Vec<_> = self
      .orders(&market.key())?
      .values()
      .collect::<Vec<&Vec<DlobNode>>>()
      .par_iter()
      .map(|o| {
        o.par_iter()
          .filter(|o| o.is_bid())
          .flat_map(|o| o.bid(cache))
          .collect::<Vec<OrderInfo>>()
      })
      .flatten()
      .collect();

    // sort so bids have the highest price first
    bids.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap());
    // highest slot in bids
    let bid_max_slot = bids.iter().map(|b| b.slot).max().unwrap_or(0);

    let mut asks: Vec<_> = self
      .orders(&market.key())?
      .values()
      .collect::<Vec<&Vec<DlobNode>>>()
      .par_iter()
      .map(|o| {
        o.par_iter()
          .filter(|o| o.is_ask())
          .flat_map(|o| o.ask(cache))
          .collect::<Vec<OrderInfo>>()
      })
      .flatten()
      .collect();

    // sort so asks have the lowest price first
    asks.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());
    let ask_max_slot = asks.iter().map(|b| b.slot).max().unwrap_or(0);

    let oracle_price = DriftUtils::oracle_price(market, cache, None)?;

    let slot = bid_max_slot.max(ask_max_slot);

    let best_bid = bids
      .iter()
      .filter(|b| b.price < oracle_price)
      .max_by(|a, b| a.price.partial_cmp(&b.price).unwrap())
      .ok_or(anyhow::anyhow!("No bids found"))?;

    let best_ask = asks
      .iter()
      .filter(|b| b.price > oracle_price)
      .min_by(|a, b| a.price.partial_cmp(&b.price).unwrap())
      .ok_or(anyhow::anyhow!("No asks found"))?;
    let spread = best_ask.price - best_bid.price;
    if spread < 0.0 {
      return Err(anyhow::anyhow!("Spread is negative"));
    }

    Ok(L3Orderbook {
      bids,
      asks,
      spread,
      slot,
      oracle_price,
    })
  }

  pub fn orders(&self, market: &MarketKey) -> anyhow::Result<&HashMap<UserKey, Vec<DlobNode>>> {
    self
      .orderbook
      .get(market)
      .ok_or(anyhow::anyhow!("Orderbook not found for market {}", market))
  }

  pub fn insert_order(
    &mut self,
    market: MarketKey,
    user: UserKey,
    order: Order,
  ) -> anyhow::Result<()> {
    let node = DlobNode::new(order);
    if !node.filled() {
      self
        .orderbook
        .entry(market)
        .or_default()
        .entry(user)
        .or_default()
        .push(node);
    } else {
      let orders = self
        .orderbook
        .entry(market)
        .or_default()
        .entry(user)
        .or_default();
      orders.retain(|o| o.order.order_id != node.order.order_id);
    }
    Ok(())
  }

  pub fn insert_user(&mut self, user: DecodedAcctCtx<User>) -> anyhow::Result<()> {
    for o in user.decoded.orders {
      let market = MarketId::from((o.market_index, o.market_type)).key();
      self.insert_order(market, user.key, o)?;
    }
    Ok(())
  }

  pub fn load(&mut self, users: Vec<DecodedAcctCtx<User>>) -> anyhow::Result<()> {
    let markets = Arc::new(self.markets.clone());
    let results: Vec<(UserKey, Vec<Order>)> = users
      .into_par_iter()
      .flat_map(|u| {
        if u.decoded.has_open_order {
          let orders = u
            .decoded
            .orders
            .into_par_iter()
            .flat_map(|o| {
              if markets.contains(&MarketId::from((o.market_index, o.market_type))) {
                Some(o)
              } else {
                None
              }
            })
            .collect::<Vec<Order>>();
          Some((u.key, orders))
        } else {
          None
        }
      })
      .collect();
    for (user, orders) in results {
      for o in orders {
        let market = MarketId::from((o.market_index, o.market_type)).key();
        self.insert_order(market, user, o)?;
      }
    }
    Ok(())
  }

  pub async fn load_from_rpc(&mut self, rpc: &RpcClient) -> anyhow::Result<()> {
    let users = DriftUtils::users(rpc).await?;
    for u in users {
      self.insert_user(u)?;
    }
    Ok(())
  }
}
