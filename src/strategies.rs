use anyhow::bail;
use anyhow::Result;
use std::collections::HashMap;
use std::iter::Iterator;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::positions::tt_api;

use super::account::Account;
use super::mktdata::MktData;
use super::orders::Orders;
use super::positions::OptionStrategy;
use super::web_client::WebClient;
// use super::orders::Orders;

trait Strategy: Sync + Send {
    fn should_exit(&self, mktdata: &MktData) -> Result<bool>;
    fn get_symbol(&self) -> &str;
}

struct CreditSpread {
    position: OptionStrategy,
}

impl CreditSpread {
    fn new(position: OptionStrategy) -> Self {
        Self { position }
    }
}

impl Strategy for CreditSpread {
    fn should_exit(&self, mktdata: &MktData) -> anyhow::Result<bool> {
        if let Some(leg) = self.position.legs.first() {
            let snapshot = mktdata.get_snapshot(leg.symbol());
            // if leg.delta > leg.delta + 15 {
            //     return Ok(true);
            // }

            // if orders.get_order().premimum % 2 < orders.orders.get_order().pnl {
            //     return true;
            // }

            return anyhow::Ok(false);
        }
        bail!("Safe")
    }

    fn get_symbol(&self) -> &str {
        self.position.legs.first().unwrap().symbol()
    }
}

pub(crate) struct Strategies {}

impl Strategies {
    pub async fn new(web_client: Arc<WebClient>, cancel_token: CancellationToken) -> Result<Self> {
        let _account = Account::new(Arc::clone(&web_client), cancel_token.clone());
        let mktdata = MktData::new(Arc::clone(&web_client), cancel_token.clone());
        let orders = Orders::new(Arc::clone(&web_client), cancel_token.clone());
        tokio::spawn(async move {
            let mut strategies = match Self::get_strategies(&web_client).await {
                Ok(val) => val,
                Err(err) => panic!("Failed to pull stragies on initialisation, error: {}", err),
            };

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
                                cancel_token.cancel();
                                break
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
        strategies: &[Box<dyn Strategy>],
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

    async fn get_strategies(web_client: &WebClient) -> Result<Vec<Box<dyn Strategy>>> {
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
        Ok(Self::convert_api_data_into_strategies(positions.data.legs).await)
    }

    async fn convert_api_data_into_strategies<'a>(
        legs: Vec<tt_api::Leg>,
    ) -> Vec<Box<dyn Strategy>> {
        let mut sorted_legs: HashMap<String, Vec<tt_api::Leg>> = HashMap::new();

        legs.iter().for_each(|leg| {
            let underlying = leg.underlying_symbol.clone().unwrap(); // Assuming underlying_symbol is a string
            sorted_legs.entry(underlying).or_default().push(leg.clone());
        });

        let strats = sorted_legs
            .values()
            .map(|legs| {
                let spread = OptionStrategy::new(legs.clone());
                Box::new(CreditSpread::new(spread)) as Box<dyn Strategy>
            })
            .collect();

        strats
    }
}
