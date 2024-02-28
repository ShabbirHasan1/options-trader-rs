use anyhow::bail;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::info;
use tracing::warn;

use super::positions::tt_api;
use crate::positions::ComplexSymbol;

use super::account::Account;
use super::mktdata::MktData;
use super::orders::Orders;
use super::positions::OptionStrategy;
use super::web_client::WebClient;
// use super::orders::Orders;

trait Strategy {
    fn should_exit(&self, mktdata: &MktData) -> bool;
    fn get_symbol(&self) -> &str;
}

struct CreditSpread {
    position: OptionStrategy<ComplexSymbol>,
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
        let mut strategies = Self::get_strategies(&web_client).await?;
        tokio::spawn(async move {
            Self::subscribe_to_updates(&strategies, &mktdata, &cancel_token).await;
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        break
                    }
                    _ = sleep(Duration::from_secs(5)) => {
                        strategies.iter().for_each(|strategy| Self::check_stops(&strategy, &mktdata, &orders));
                    }
                    _ = sleep(Duration::from_secs(30)) => {
                        strategies = match Self::get_strategies(&web_client).await {
                            Ok(val) => {
                                Self::subscribe_to_updates(&val, &mktdata, &cancel_token).await;
                                val
                            }
                            Err(err) => {
                                error!("Failed to pull positions from broker, error: {}", err);
                                cancel_token.cancel()
                            }
                        }
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
        strategies: &[Box<dyn Strategy + Send + Sync + 'static>],
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
    ) -> Result<Vec<Box<dyn Strategy + Send + Sync + 'static>>> {
        let positions = match web_client
            .get::<tt_api::AccountPositions>(
                format!("accounts/{}/positions", web_client.get_account()).as_str(),
            )
            .await
        {
            Ok(val) => val,
            Err(err) => {
                bail!(
                    "Failed to refresh position data from broker, error: {}",
                    err
                )
            }
        };
        Ok(Self::collect_strategies(positions).await)
    }

    async fn collect_strategies(
        legs: Vec<tt_api::Leg>,
    ) -> Vec<Box<dyn Strategy + Send + Sync + 'static>> {
        let mut sorted_legs: HashMap<String, Vec<tt_api::Leg>> = HashMap::new();
        for leg in legs {
            let underlying = leg.underlying_symbol.clone(); // Assuming underlying_symbol is a string
            sorted_legs
                .entry(underlying)
                .or_insert(Vec::new())
                .push(leg);
        }

        sorted_legs
            .values()
            .map(|legs| Box::new(OptionStrategy::<ComplexSymbol>::new(legs)))
            .collect()
    }
}
