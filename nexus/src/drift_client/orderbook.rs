use std::collections::HashMap;

use drift_cpi::{OracleSource, Order, User};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;

use crate::{BidAsk, DecodedAcctCtx, DlobNode, DriftUtils, L3Orderbook, MarketId, ToAccountInfo};

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

  pub fn l3(
    &self,
    market: MarketKey,
    src: &OracleSource,
    key: Pubkey,
    acct: &impl ToAccountInfo,
    slot: u64,
  ) -> anyhow::Result<L3Orderbook> {
    let mut bids: Vec<BidAsk> = self
      .orders(market)?
      .values()
      .filter(|o| o.is_bid())
      .flat_map(|o| o.bid(src, key, acct, slot))
      .collect();
    // sort so bids have the highest price first
    bids.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap());

    let mut asks: Vec<BidAsk> = self
      .orders(market)?
      .values()
      .filter(|o| o.is_ask())
      .flat_map(|o| o.ask(src, key, acct, slot))
      .collect();
    // sort so asks have the lowest price first
    asks.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());

    let best_bid = bids.first().ok_or(anyhow::anyhow!("No bids"))?;
    let best_ask = asks.first().ok_or(anyhow::anyhow!("No asks"))?;
    let spread = best_ask.price - best_bid.price;
    if spread < 0.0 {
      return Err(anyhow::anyhow!("Spread is negative"));
    }

    Ok(L3Orderbook {
      bids,
      asks,
      spread,
      slot,
    })
  }

  pub fn orders(&self, market: MarketKey) -> anyhow::Result<&HashMap<UserKey, DlobNode>> {
    self
      .orderbook
      .get(&market)
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

  pub async fn load(&mut self, rpc: &RpcClient) -> anyhow::Result<()> {
    self.load_all_users(rpc).await?;
    Ok(())
  }

  async fn load_all_users(&mut self, rpc: &RpcClient) -> anyhow::Result<()> {
    let users = DriftUtils::users(rpc).await?;
    for u in users {
      self.insert_user(u);
    }
    Ok(())
  }
}
