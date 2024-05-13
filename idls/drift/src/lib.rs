#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(clippy::too_many_arguments)]

use once_cell::sync::Lazy;

use common::decode_account;

pub mod oracle;
pub mod math;
pub mod casting;
pub mod safe_math;
pub mod safe_unwrap;
pub mod ceil_div;
pub mod floor_div;

anchor_gen::generate_cpi_crate!("idl.json");
anchor_lang::declare_id!("dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH");

pub static PATH: Lazy<String> = Lazy::new(|| env!("CARGO_MANIFEST_DIR").to_string());
pub static PROGRAM_NAME: Lazy<String> = Lazy::new(|| PATH.split('/').last().unwrap().to_string());
pub static PROGRAM_ID: Lazy<Pubkey> = Lazy::new(|| ID);

decode_account!(
    pub enum AccountType {
        PhoenixV1FulfillmentConfig(PhoenixV1FulfillmentConfig),
        SerumV3FulfillmentConfig(SerumV3FulfillmentConfig),
        InsuranceFundStake(InsuranceFundStake),
        ProtocolIfSharesTransferConfig(ProtocolIfSharesTransferConfig),
        PerpMarket(PerpMarket),
        SpotMarket(SpotMarket),
        State(State),
        User(User),
        UserStats(UserStats),
        ReferrerName(ReferrerName),
    }
);

// initializeUser
// initializeUserStats
// initializeReferrerName
// deposit
// withdraw
// transferDeposit
// placePerpOrder
// cancelOrder
// cancelOrderByUserId
// cancelOrders
// cancelOrdersByIds
// modifyOrder
// modifyOrderByUserId
// placeAndTakePerpOrder
// placeAndMakePerpOrder
// placeSpotOrder
// placeAndTakeSpotOrder
// placeAndMakeSpotOrder
// placeOrders
// beginSwap
// endSwap
// addPerpLpShares
// removePerpLpShares
// removePerpLpSharesInExpiringMarket
// updateUserName
// updateUserCustomMarginRatio
// updateUserMarginTradingEnabled
// updateUserDelegate
// updateUserReduceOnly
// updateUserAdvancedLp
// deleteUser
// reclaimRent
// fillPerpOrder
// revertFill
// fillSpotOrder
// triggerOrder
// forceCancelOrders
// updateUserIdle
// updateUserOpenOrdersCount
// adminDisableUpdatePerpBidAskTwap
// settlePnl
// settleFundingPayment
// settleLp
// settleExpiredMarket
// liquidatePerp
// liquidateSpot
// liquidateBorrowForPerpPnl
// liquidatePerpPnlForDeposit
// resolvePerpPnlDeficit
// resolvePerpBankruptcy
// resolveSpotBankruptcy
// settleRevenueToInsuranceFund
// updateFundingRate
// updatePrelaunchOracle
// updatePerpBidAskTwap
// updateSpotMarketCumulativeInterest
// updateAmms
// updateSpotMarketExpiry
// updateUserQuoteAssetInsuranceStake
// initializeInsuranceFundStake
// addInsuranceFundStake
// requestRemoveInsuranceFundStake
// cancelRequestRemoveInsuranceFundStake
// removeInsuranceFundStake
// transferProtocolIfShares
// initialize
// initializeSpotMarket
// initializeSerumFulfillmentConfig
// updateSerumFulfillmentConfigStatus
// initializePhoenixFulfillmentConfig
// phoenixFulfillmentConfigStatus
// updateSerumVault
// initializePerpMarket
// deleteInitializedPerpMarket
// moveAmmPrice
// recenterPerpMarketAmm
// updatePerpMarketExpiry
// settleExpiredMarketPoolsToRevenuePool
// depositIntoPerpMarketFeePool
// depositIntoSpotMarketRevenuePool
// repegAmmCurve
// updatePerpMarketAmmOracleTwap
// resetPerpMarketAmmOracleTwap
// updateK
// updatePerpMarketMarginRatio
// updatePerpMarketFundingPeriod
// updatePerpMarketMaxImbalances
// updatePerpMarketLiquidationFee
// updateInsuranceFundUnstakingPeriod
// updateSpotMarketLiquidationFee
// updateWithdrawGuardThreshold
// updateSpotMarketIfFactor
// updateSpotMarketRevenueSettlePeriod
// updateSpotMarketStatus
// updateSpotMarketPausedOperations
// updateSpotMarketAssetTier
// updateSpotMarketMarginWeights
// updateSpotMarketBorrowRate
// updateSpotMarketMaxTokenDeposits
// updateSpotMarketScaleInitialAssetWeightStart
// updateSpotMarketOracle
// updateSpotMarketStepSizeAndTickSize
// updateSpotMarketMinOrderSize
// updateSpotMarketOrdersEnabled
// updateSpotMarketName
// updatePerpMarketStatus
// updatePerpMarketPausedOperations
// updatePerpMarketContractTier
// updatePerpMarketImfFactor
// updatePerpMarketUnrealizedAssetWeight
// updatePerpMarketConcentrationCoef
// updatePerpMarketCurveUpdateIntensity
// updatePerpMarketTargetBaseAssetAmountPerLp
// updatePerpMarketPerLpBase
// updateLpCooldownTime
// updatePerpFeeStructure
// updateSpotFeeStructure
// updateInitialPctToLiquidate
// updateLiquidationDuration
// updateLiquidationMarginBufferRatio
// updateOracleGuardRails
// updateStateSettlementDuration
// updateStateMaxNumberOfSubAccounts
// updateStateMaxInitializeUserFee
// updatePerpMarketOracle
// updatePerpMarketBaseSpread
// updateAmmJitIntensity
// updatePerpMarketMaxSpread
// updatePerpMarketStepSizeAndTickSize
// updatePerpMarketName
// updatePerpMarketMinOrderSize
// updatePerpMarketMaxSlippageRatio
// updatePerpMarketMaxFillReserveFraction
// updatePerpMarketMaxOpenInterest
// updatePerpMarketFeeAdjustment
// updateAdmin
// updateWhitelistMint
// updateDiscountMint
// updateExchangeStatus
// updatePerpAuctionDuration
// updateSpotAuctionDuration
// adminRemoveInsuranceFundStake
// initializeProtocolIfSharesTransferConfig
// updateProtocolIfSharesTransferConfig
// initializePrelaunchOracle
// updatePrelaunchOracleParams
// deletePrelaunchOracle

/// cargo test --package drift-cpi --lib accounts -- --exact --show-output
#[test]
fn accounts() {
  let idl_path = "idl.json";
  let idl_str = std::fs::read_to_string(idl_path).unwrap();
  let idl = serde_json::from_str::<serde_json::Value>(&idl_str).unwrap();
  let accounts = serde_json::from_value::<Vec<serde_json::Value>>(idl["accounts"].clone()).unwrap();
  for account in accounts {
    println!("{}", account["name"].as_str().unwrap());
  }
}

/// cargo test --package drift-cpi --lib instructions -- --exact --show-output
#[test]
fn instructions() {
  let idl_path = "idl.json";
  let idl_str = std::fs::read_to_string(idl_path).unwrap();
  let idl = serde_json::from_str::<serde_json::Value>(&idl_str).unwrap();
  let accounts = serde_json::from_value::<Vec<serde_json::Value>>(idl["instructions"].clone()).unwrap();
  for account in accounts {
    println!("{}", account["name"].as_str().unwrap());
  }
}