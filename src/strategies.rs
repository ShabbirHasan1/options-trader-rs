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

use crate::mktdata::tt_api as mktd_api;
use crate::mktdata::tt_api::CandleData;
use crate::positions::tt_api as pos_api;
use crate::positions::InstrumentType;
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
    fn get_underlying(&self) -> &str;
    fn get_symbols(&self) -> Vec<&str>;
    fn get_instrument_type(&self) -> InstrumentType;
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
            &self.position.legs.first().unwrap().symbol(),
            &self.position
        )
    }
}

impl Strategy for CreditSpread {
    fn should_exit(&self, mktdata: &MktData) -> anyhow::Result<bool> {
        if let Some(complex_symbol) = self.position.legs.first() {
            let snapshot =
                mktdata.get_snapshot_data(complex_symbol.underlying(), complex_symbol.symbol());
            // if let Some(FeedEvent::Greeks(greeks)) = snapshot.data.first() {
            //     if greeks.delta > 0.4 {
            //         info!(
            //             "Hit stop on credit spread {}",
            //             &self.position.legs.first().unwrap().underlying()
            //         );
            //         return anyhow::Ok(true);
            //     }
            // }

            // if self.position.premimum % 2 < orders.orders.get_order().pnl {
            //     return true;
            // }

            return anyhow::Ok(false);
        }
        bail!("Something went wrong building the positions")
    }

    fn get_underlying(&self) -> &str {
        self.position.legs.first().unwrap().underlying()
    }

    fn get_symbols(&self) -> Vec<&str> {
        self.position.legs.iter().map(|leg| leg.symbol()).collect()
    }

    fn get_instrument_type(&self) -> InstrumentType {
        self.position.legs.first().unwrap().instrument_type()
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
            &self.position.legs.first().unwrap().symbol(),
            &self.position
        )
    }
}

impl Strategy for CalendarSpread {
    fn should_exit(&self, mktdata: &MktData) -> anyhow::Result<bool> {
        if let Some(complex_symbol) = self.position.legs.first() {
            let snapshot =
                mktdata.get_snapshot_data(complex_symbol.underlying(), complex_symbol.symbol());
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

    fn get_underlying(&self) -> &str {
        self.position.legs.first().unwrap().underlying()
    }

    fn get_symbols(&self) -> Vec<&str> {
        self.position.legs.iter().map(|leg| leg.symbol()).collect()
    }

    fn get_instrument_type(&self) -> InstrumentType {
        self.position.legs.first().unwrap().instrument_type()
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
        if let Some(complex_symbol) = self.position.legs.first() {
            let snapshot =
                mktdata.get_snapshot_data(complex_symbol.underlying(), complex_symbol.symbol());
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

    fn get_underlying(&self) -> &str {
        self.position.legs.first().unwrap().underlying()
    }

    fn get_symbols(&self) -> Vec<&str> {
        self.position.legs.iter().map(|leg| leg.symbol()).collect()
    }

    fn get_instrument_type(&self) -> InstrumentType {
        self.position.legs.first().unwrap().instrument_type()
    }

    fn print(&self) {
        info!("{}", &self);
    }
}

pub(crate) struct Strategies {}

impl Strategies {
    pub async fn new(web_client: Arc<WebClient>, cancel_token: CancellationToken) -> Result<Self> {
        let _account = Account::new(Arc::clone(&web_client), cancel_token.clone());
        let mut mktdata = MktData::new(Arc::clone(&web_client), cancel_token.clone());
        let orders = Orders::new(Arc::clone(&web_client), cancel_token.clone());
        let mut strategies = match Self::get_strategies(&web_client).await {
            Ok(val) => val,
            Err(err) => bail!(
                "Failed to pull strategies on initialisation, error: {}",
                err
            ),
        };
        Self::subscribe_to_updates(&strategies, &mut mktdata, &cancel_token).await;

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        break
                    }
                    _ = sleep(Duration::from_secs(30)) => {
                        strategies = match Self::get_strategies(&web_client).await {
                            Ok(val) => {
                                Self::subscribe_to_updates(&val, &mut mktdata, &cancel_token).await;
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
        mktdata: &mut MktData,
        _cancel_token: &CancellationToken,
    ) {
        for strategy in strategies {
            if let Err(err) = mktdata
                .subscribe_to_mktdata(
                    strategy.get_underlying(),
                    strategy.get_symbols(),
                    strategy.get_instrument_type(),
                )
                .await
            {
                error!(
                    "Failed to subscribe to mktdata for symbol: {}, error: {}",
                    strategy.get_symbols().join(", "),
                    err
                );
                //TODO: Should we restart the app
                //cancel_token.cancel();
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
            .get::<pos_api::AccountPositions>(
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

    async fn convert_api_data_into_strategies(legs: Vec<pos_api::Leg>) -> Vec<Box<dyn Strategy>> {
        let mut sorted_legs: HashMap<String, Vec<pos_api::Leg>> = HashMap::new();

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
