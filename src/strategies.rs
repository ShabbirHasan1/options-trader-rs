use anyhow::bail;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::info;
use tracing::warn;

use super::account::Account;
use super::mktdata::MktData;
use super::orders::Orders;
use super::web_client::WebClient;
// use super::orders::Orders;

mod tt_api {
    use super::*;

    #[derive(Debug, Deserialize, Serialize)]
    pub struct AccountPositions {
        pub data: Positions,
        pub context: String,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct Positions {
        #[serde(rename = "items")]
        pub positions: Vec<Position>,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct Position {
        #[serde(rename = "instrument-type")]
        pub instrument_type: Option<String>,
        pub multiplier: Option<i32>,
        #[serde(rename = "realized-today")]
        pub realized_today: Option<String>,
        #[serde(rename = "is-frozen")]
        pub is_frozen: bool,
        #[serde(rename = "updated-at")]
        pub updated_at: Option<String>,
        #[serde(rename = "average-daily-market-close-price")]
        pub average_daily_market_close_price: Option<String>,
        #[serde(rename = "deliverable-type")]
        pub deliverable_type: Option<String>,
        #[serde(rename = "underlying-symbol")]
        pub underlying_symbol: Option<String>,
        #[serde(rename = "mark-price")]
        pub mark_price: Option<String>,
        #[serde(rename = "account-number")]
        pub account_number: Option<String>,
        #[serde(rename = "fixing-price")]
        pub fixing_price: Option<String>,
        pub quantity: i32,
        #[serde(rename = "realized-day-gain-date")]
        pub realized_day_gain_date: Option<String>,
        #[serde(rename = "expires-at")]
        pub expires_at: Option<String>,
        pub mark: Option<Option<String>>,
        #[serde(rename = "realized-day-gain")]
        pub realized_day_gain: Option<String>,
        #[serde(rename = "realized-day-gain-effect")]
        pub realized_day_gain_effect: Option<String>,
        #[serde(rename = "cost-effect")]
        pub cost_effect: Option<String>,
        #[serde(rename = "close-price")]
        pub close_price: Option<String>,
        #[serde(rename = "average-yearly-market-close-price")]
        pub average_yearly_market_close_price: Option<String>,
        #[serde(rename = "average-open-price")]
        pub average_open_price: Option<String>,
        #[serde(rename = "is-suppressed")]
        pub is_suppressed: bool,
        pub created_at: Option<String>,
        pub symbol: String,
        #[serde(rename = "realized-today-date")]
        pub realized_today_date: Option<String>,
        #[serde(rename = "order-id")]
        pub order_id: Option<String>,
        #[serde(rename = "realized-today-effect")]
        pub realized_today_effect: Option<String>,
        #[serde(rename = "quantity-direction")]
        pub quantity_direction: Option<String>,
        #[serde(rename = "restricted-quantity")]
        pub restricted_quantity: Option<i32>,
    }
}

trait Strategy {
    fn should_exit(&self, mktdata: &MktData) -> bool;
    fn get_symbol(&self) -> &str;
}

struct CreditSpread {
    position: tt_api::Position,
}

impl CreditSpread {
    fn new(position: tt_api::Position) -> Self {
        Self { position }
    }
}

impl Strategy for CreditSpread {
    fn should_exit(&self, mktdata: &MktData) -> bool {
        let snapshot = mktdata.get_snapshot(&self.position.symbol);
        // if snapshot.leg[0].delta > leg[0].delta + 15 {
        //     return true;
        // }

        // if orders.get_order().premimum % 2 < orders.orders.get_order().pnl {
        //     return true;
        // }

        return false;
    }
    fn get_symbol(&self) -> &str {
        &self.position.symbol
    }
}

pub(crate) struct Strategies {}

impl Strategies {
    pub async fn new(web_client: Arc<WebClient>, cancel_token: CancellationToken) -> Result<Self> {
        let _account = Account::new(Arc::clone(&web_client), cancel_token.clone());
        let mktdata = MktData::new(Arc::clone(&web_client), cancel_token.clone());
        let orders = Orders::new(Arc::clone(&web_client), cancel_token.clone());
        let mut receiver = web_client.subscribe_to_events();
        tokio::spawn(async move {
            let mut strategies = Self::get_strategies(&web_client, Vec::default()).await;
            Self::subscribe_to_updates(&strategies, &mktdata, &cancel_token).await;
            loop {
                tokio::select! {
                    msg = receiver.recv() => {
                        match msg {
                            Err(RecvError::Lagged(err)) => warn!("Publisher channel skipping a number of messages: {}", err),
                            Err(RecvError::Closed) => {
                                error!("Publisher channel closed");
                                cancel_token.cancel();
                            }
                            std::result::Result::Ok(val) => {
                                match Self::handle_msg(val, &cancel_token) {
                                    Ok(val) => info!("All good {:?}", val),
                                    _ => ()
                                }
                            }
                        }
                    }
                    _ = cancel_token.cancelled() => {
                        break
                    }
                    _ = sleep(Duration::from_secs(5)) => {
                        strategies.iter().for_each(|strategy| Self::check_stops(&strategy, &mktdata, &orders));
                    }
                    _ = sleep(Duration::from_secs(30)) => {
                        strategies = Self::get_strategies(&web_client, strategies).await;
                        Self::subscribe_to_updates(&strategies, &mktdata, &cancel_token).await;
                    }
                    _ = cancel_token.cancelled() => {
                        break
                    }
                }
            }
        });
        Ok(Self {})
    }

    fn handle_msg(msg: String, cancel_token: &CancellationToken) -> Result<tt_api::Position> {
        match serde_json::from_str::<tt_api::Position>(&msg) {
            Ok(val) => Ok(val),
            Err(err) => {
                bail!("Failed to deserialize position, error: {}", err)
            }
        }
    }

    async fn subscribe_to_updates(
        strategies: &Vec<Box<dyn Strategy + Send + Sync + 'static>>,
        mktdata: &MktData,
        cancel_token: &CancellationToken,
    ) {
        for strategy in strategies {
            if let Err(err) = mktdata.subscribe_to_mktdata(strategy.get_symbol()).await {
                error!(
                    "Failed to subscribe to mktdata for symbol: {}, error: {}",
                    strategy.get_symbol(),
                    err
                );
                cancel_token.cancel();
            }
        }
    }

    fn check_stops<Strategy>(strategy: &Strategy, _mktdata: &MktData, orders: &Orders) {
        let _ = strategy;
        // strategies.iter.map()
        orders.liquidate_position();
    }

    async fn get_strategies(
        web_client: &WebClient,
        current: Vec<Box<dyn Strategy + Send + Sync + 'static>>,
    ) -> Vec<Box<dyn Strategy + Send + Sync + 'static>> {
        // fn to_strategy(position: tt_api::Position) -> Box<dyn Strategy + Send + Sync + 'static> {
        //     Box::new(CreditSpread::new(position))
        // }

        // let me = match web_client
        //     .get::<tt_api::Position>(format!("customers/me").as_str())
        //     .await
        // {
        //     Ok(val) => val,
        //     Err(err) => {
        //         warn!("Failed to refresh me data from broker, error: {}", err);
        //         return current;
        //     }
        // };
        // let account_me = match web_client
        //     .get::<tt_api::Position>(format!("customers/me/accounts").as_str())
        //     .await
        // {
        //     Ok(val) => val,
        //     Err(err) => {
        //         warn!(
        //             "Failed to refresh position data from broker, error: {}",
        //             err
        //         );
        //         return current;
        //     }
        // };
        let positions = match web_client
            .get::<tt_api::AccountPositions>(
                format!("accounts/{}/positions", web_client.get_account()).as_str(),
            )
            .await
        {
            Ok(val) => val,
            Err(err) => {
                warn!(
                    "Failed to refresh position data from broker, error: {}",
                    err
                );
                return current;
            }
        };
        // positions.iter().map(|p| to_strategy(p.clone())).collect();
        Vec::new()
    }
}
