use crate::graphql::{GraphqlRequest, GraphqlResponse};
use crate::DriftUsers;
use drift_cpi::User;
use reqwest::Client;

pub struct GraphqlClient {
  client: Client,
  url: String,
}

impl GraphqlClient {
  pub fn new(url: String) -> Self {
    Self {
      client: Client::new(),
      url,
    }
  }

  pub async fn drift_users(&self) -> anyhow::Result<Vec<User>> {
    let op_name = "DriftQuery";
    let operations_doc = r#"
      query DriftUserQuery {
        drift_User {
          authority
          cumulativePerpFunding
          cumulativeSpotFees
          delegate
          hasOpenAuction
          hasOpenOrder
          idle
          isMarginTradingEnabled
          lastActiveSlot
          lastAddPerpLpSharesTs
          liquidationMarginFreed
          maxMarginRatio
          nextLiquidationId
          nextOrderId
          openAuctions
          openOrders
          settledPerpPnl
          status
          subAccountId
          totalDeposits
          totalSocialLoss
          totalWithdraws
          orders
          padding
          perpPositions
          pubkey
          spotPositions
          name
        }
      }
    "#;
    let body = GraphqlRequest {
      variables: (),
      query: operations_doc,
      operation_name: op_name,
    };
    let res = self.client.post(&self.url).json(&body).send().await?;

    // let str = res.text().await?;
    // let str = &str[..625];
    // log::info!("{:?}", str);

    let data: GraphqlResponse<DriftUsers> = res.json().await?;
    log::info!("{:?}", data);

    if let Some(errors) = &data.errors {
      for error in errors {
        log::error!("Graphql error: {:?}", error);
      }
    }
    let users: Vec<User> = data
      .data
      .drift_user
      .into_iter()
      .flat_map(|u| u.try_into())
      .collect();
    Ok(users)
  }
}
