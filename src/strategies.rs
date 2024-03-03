use anyhow::bail;
use anyhow::Result;
use std::collections::HashMap;
use std::fmt;
use std::iter::Iterator;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::positions::tt_api;
use crate::positions::OptionType;
use crate::positions::StrategyType;

use super::account::Account;
use super::mktdata::MktData;
use super::orders::Orders;
use super::positions::OptionStrategy;
use super::web_client::WebClient;
// use super::orders::Orders;

trait Strategy: Sync + Send {
    fn should_exit(&self, mktdata: &MktData) -> Result<bool>;
    fn get_symbol(&self) -> &str;
    fn print(&self);
}

struct CreditSpread {
    position: OptionStrategy,
}

impl CreditSpread {
    fn new(position: OptionStrategy) -> Self {
        Self { position }
    }
}
impl fmt::Display for CreditSpread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CreditSpread {}: [{}\n]",
            &self.position.legs.first().unwrap().underlying(),
            &self.position
        )
    }
}

impl Strategy for CreditSpread {
    fn should_exit(&self, mktdata: &MktData) -> anyhow::Result<bool> {
        if let Some(leg) = self.position.legs.first() {
            let snapshot = mktdata.get_snapshot(leg.symbol());
            // if let Some(delta) = snapshot.delta {
            //     if leg.delta > 40 {
            //         return anyhow::Ok(true);
            //     }
            // }

            // if orders.get_order().premimum % 2 < orders.orders.get_order().pnl {
            //     return true;
            // }

            return anyhow::Ok(false);
        }
        bail!("Something went wrong building the positions")
    }

    fn get_symbol(&self) -> &str {
        self.position.legs.first().unwrap().symbol()
    }

    fn print(&self) {
        info!("{}", &self);
    }
}

struct CalendarSpread {
    position: OptionStrategy,
}

impl CalendarSpread {
    fn new(position: OptionStrategy) -> Self {
        Self { position }
    }
}
impl fmt::Display for CalendarSpread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CalendarSpread {}: [{}\n]",
            &self.position.legs.first().unwrap().underlying(),
            &self.position
        )
    }
}

impl Strategy for CalendarSpread {
    fn should_exit(&self, mktdata: &MktData) -> anyhow::Result<bool> {
        if let Some(leg) = self.position.legs.first() {
            let snapshot = mktdata.get_snapshot(leg.symbol());
            // if let Some(delta) = snapshot.delta {
            //     if leg.delta > 40 {
            //         return anyhow::Ok(true);
            //     }
            // }

            // if orders.get_order().premimum % 2 < orders.orders.get_order().pnl {
            //     return true;
            // }

            return anyhow::Ok(false);
        }
        bail!("Something went wrong building the positions")
    }

    fn get_symbol(&self) -> &str {
        self.position.legs.first().unwrap().symbol()
    }

    fn print(&self) {
        info!("{}", &self);
    }
}

struct IronCondor {
    position: OptionStrategy,
}

impl IronCondor {
    fn new(position: OptionStrategy) -> Self {
        Self { position }
    }
}
impl fmt::Display for IronCondor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "IronCondor {}: [{}\n]",
            &self.position.legs.first().unwrap().underlying(),
            &self.position
        )
    }
}

impl Strategy for IronCondor {
    fn should_exit(&self, mktdata: &MktData) -> anyhow::Result<bool> {
        if let Some(leg) = self.position.legs.first() {
            let snapshot = mktdata.get_snapshot(leg.symbol());
            // if let Some(delta) = snapshot.delta {
            //     if leg.delta > 40 {
            //         return anyhow::Ok(true);
            //     }
            // }

            // if orders.get_order().premimum % 2 < orders.orders.get_order().pnl {
            //     return true;
            // }

            return anyhow::Ok(false);
        }
        bail!("Something went wrong building the positions")
    }

    fn get_symbol(&self) -> &str {
        self.position.legs.first().unwrap().symbol()
    }

    fn print(&self) {
        info!("{}", &self);
    }
}

pub(crate) struct Strategies {}

impl Strategies {
    pub async fn new(web_client: Arc<WebClient>, cancel_token: CancellationToken) -> Result<Self> {
        let _account = Account::new(Arc::clone(&web_client), cancel_token.clone());
        let mktdata = MktData::new(Arc::clone(&web_client), cancel_token.clone());
        let orders = Orders::new(Arc::clone(&web_client), cancel_token.clone());
        let mut strategies = match Self::get_strategies(&web_client).await {
            Ok(val) => val,
            Err(err) => bail!(
                "Failed to pull strategies on initialisation, error: {}",
                err
            ),
        };
        Self::subscribe_to_updates(&strategies, &mktdata, &cancel_token).await;

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        break
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
                    _ = sleep(Duration::from_secs(5)) => {
                        strategies.iter().for_each(|strategy| Self::check_stops(&strategy, &mktdata, &orders));
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

    async fn convert_api_data_into_strategies(legs: Vec<tt_api::Leg>) -> Vec<Box<dyn Strategy>> {
        let mut sorted_legs: HashMap<String, Vec<tt_api::Leg>> = HashMap::new();

        legs.iter().for_each(|leg| {
            let underlying = leg.underlying_symbol.clone().unwrap(); // Assuming underlying_symbol is a string
            sorted_legs.entry(underlying).or_default().push(leg.clone());
        });

        let strats: Vec<Box<dyn Strategy>> = sorted_legs
            .values()
            .map(|legs| {
                let spread = OptionStrategy::new(legs.clone());

                match &spread.strategy_type {
                    StrategyType::CreditSpread => {
                        Box::new(CreditSpread::new(spread)) as Box<dyn Strategy>
                    }
                    StrategyType::CalendarSpread => {
                        Box::new(CalendarSpread::new(spread)) as Box<dyn Strategy>
                    }
                    StrategyType::IronCondor => {
                        Box::new(IronCondor::new(spread)) as Box<dyn Strategy>
                    }
                    _ => Box::new(CreditSpread::new(spread)) as Box<dyn Strategy>,
                }
            })
            .collect();

        strats.iter().for_each(|strategy| strategy.print());
        strats
    }
}
