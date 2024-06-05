use std::collections::HashMap;

use drift_cpi::{Order, User};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;

use crate::drift_client::{BidAsk, DlobNode, DriftUtils, L3Orderbook, MarketId};
use crate::DecodedAcctCtx;

type MarketKey = Pubkey;
type UserKey = Pubkey;

#[derive(Default)]
pub struct Orderbook {
  orderbook: HashMap<MarketKey, HashMap<UserKey, DlobNode>>,
}

impl Orderbook {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn ready(&self, market: &MarketKey) -> bool {
    !self
      .orderbook
      .get(market)
      .unwrap_or(&HashMap::new())
      .is_empty()
  }

  pub fn l3(&self, market: &MarketKey, oracle_price: f64) -> anyhow::Result<L3Orderbook> {
    let mut bids: Vec<BidAsk> = self
      .orders(market)?
      .values()
      .filter(|o| o.is_bid())
      .flat_map(|o| o.bid(oracle_price))
      .collect();
    // sort so bids have the highest price first
    bids.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap());
    // highest slot in bids
    let bid_max_slot = bids.iter().map(|b| b.slot).max().unwrap_or(0);

    let mut asks: Vec<BidAsk> = self
      .orders(market)?
      .values()
      .filter(|o| o.is_ask())
      .flat_map(|o| o.ask(oracle_price))
      .collect();
    // sort so asks have the lowest price first
    asks.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());
    let ask_max_slot = asks.iter().map(|b| b.slot).max().unwrap_or(0);

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

  pub fn orders(&self, market: &MarketKey) -> anyhow::Result<&HashMap<UserKey, DlobNode>> {
    self
      .orderbook
      .get(market)
      .ok_or(anyhow::anyhow!("Orderbook not found for market {}", market))
  }

  pub fn insert_order(&mut self, market: MarketKey, user: UserKey, order: Order) {
    let node = DlobNode::new(order);
    if !node.filled() {
      self.orderbook.entry(market).or_default().insert(user, node);
    } else {
      self.orderbook.entry(market).or_default().remove(&user);
    }
  }

  pub fn insert_user(&mut self, user: DecodedAcctCtx<User>) {
    for o in user.decoded.orders {
      let market = MarketId::from((o.market_index, o.market_type)).key();
      self.insert_order(market, user.key, o);
    }
  }

  pub fn load(&mut self, users: Vec<DecodedAcctCtx<User>>) -> anyhow::Result<()> {
    for u in users {
      self.insert_user(u);
    }
    Ok(())
  }

  pub async fn load_from_rpc(&mut self, rpc: &RpcClient) -> anyhow::Result<()> {
    let users = DriftUtils::users(rpc).await?;
    for u in users {
      self.insert_user(u);
    }
    Ok(())
  }
}
