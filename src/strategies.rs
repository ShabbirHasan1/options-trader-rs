use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::warn;

use crate::web_client;
use crate::web_client::http_client::HttpClient;

use super::mktdata::MktData;
use super::orders::Orders;
use super::web_client::WebClient;
// use super::orders::Orders;

mod tt_api {
    use super::*;

    #[derive(Debug, Deserialize, Serialize)]
    pub struct Positions {
        pub positions: Vec<Position>,
    }
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct Position {
        pub instrument_type: String,
        pub multiplier: i32,
        pub realized_today: i32,
        pub is_frozen: bool,
        pub updated_at: String,
        pub average_daily_market_close_price: i32,
        pub deliverable_type: String,
        pub underlying_symbol: String,
        pub mark_price: i32,
        pub account_number: String,
        pub fixing_price: i32,
        pub quantity: i32,
        pub realized_day_gain_date: String,
        pub expires_at: String,
        pub mark: i32,
        pub realized_day_gain: i32,
        pub realized_day_gain_effect: String,
        pub cost_effect: String,
        pub close_price: i32,
        pub average_yearly_market_close_price: i32,
        pub average_open_price: i32,
        pub is_suppressed: bool,
        pub created_at: String,
        pub symbol: String,
        pub realized_today_date: String,
        pub order_id: i32,
        pub realized_today_effect: String,
        pub quantity_direction: String,
        pub restricted_quantity: i32,
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
        let mktdata = MktData::new(Arc::clone(&web_client), cancel_token.clone());
        let orders = Orders::new(Arc::clone(&web_client));
        tokio::spawn(async move {
            let mut strategies = Self::get_strategies(&web_client, Vec::default()).await;
            Self::subscribe_to_updates(&strategies, &mktdata, &cancel_token).await;

            loop {
                tokio::select! {
                    _ = sleep(Duration::from_secs(5)) => {
                        strategies.iter().for_each(|strategy| Self::check_stops(strategy, &mktdata, &orders));
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

    fn check_stops(
        strategy: &Box<dyn Strategy + Send + Sync + 'static>,
        _mktdata: &MktData,
        orders: &Orders,
    ) {
        // strategies.iter.map()
    }

    async fn get_strategies(
        web_client: &WebClient,
        current: Vec<Box<dyn Strategy + Send + Sync + 'static>>,
    ) -> Vec<Box<dyn Strategy + Send + Sync + 'static>> {
        fn to_strategy(position: tt_api::Position) -> Box<dyn Strategy + Send + Sync + 'static> {
            Box::new(CreditSpread::new(position))
        }

        let positions = match web_client
            .get::<tt_api::Positions>(
                format!(
                    "accounts/{}/positions?net-positions=true",
                    web_client.get_account()
                )
                .as_str(),
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
        positions
            .positions
            .iter()
            .map(|p| to_strategy(p.clone()))
            .collect()
    }
}
