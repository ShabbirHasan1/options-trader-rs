use anyhow::bail;
use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::fmt;
use std::iter::Iterator;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::info;

use super::generic::*;
use super::spx::SpxStrategy;
use crate::connectivity::web_client::WebClient;
use crate::platform::{account::Account, mktdata::*, orders::*, positions::*};
use crate::tt_api::{mktdata::Quote, positions::AccountPositions, positions::Leg};

enum Strategy {
    Calendar(CalendarSpread),
    Credit(CreditSpread),
    Condor(IronCondor),
    NotTracked,
}

pub(crate) struct Strategies {
    mktdata: Arc<RwLock<MktData>>,
    orders: Arc<RwLock<Orders>>,
}

impl Strategies {
    pub async fn new(web_client: Arc<WebClient>, cancel_token: CancellationToken) -> Result<Self> {
        let _account = Account::new(Arc::clone(&web_client), cancel_token.clone());
        let mktdata = Arc::new(RwLock::new(MktData::new(
            Arc::clone(&web_client),
            cancel_token.clone(),
        )));
        let orders = Arc::new(RwLock::new(Orders::new(
            Arc::clone(&web_client),
            Arc::clone(&mktdata),
            cancel_token.clone(),
        )));
        let mut strategies = match Self::get_strategies(&web_client).await {
            Ok(val) => val,
            Err(err) => bail!(
                "Failed to pull strategies on initialisation, error: {}",
                err
            ),
        };
        Self::subscribe_to_updates(&strategies, &mktdata).await;

        let mktdata_cpy = Arc::clone(&mktdata);
        let orders_cpy = Arc::clone(&orders);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        break
                    }
                    _ = sleep(Duration::from_secs(30)) => {
                        strategies = match Self::get_strategies(&web_client).await {
                            Ok(val) => {
                                Self::subscribe_to_updates(&val, &mktdata).await;
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
                        for strategy in &strategies {
                            if let Err(err) = Self::check_stops(strategy, &mktdata, &orders).await {
                                error!("Issue checking stops, error: {}", err);
                            }
                        }
                    }
                    _ = cancel_token.cancelled() => {
                        break
                    }
                }
            }
        });
        Ok(Self {
            mktdata: mktdata_cpy,
            orders: orders_cpy,
        })
    }

    pub fn get_spx(&self) -> SpxStrategy {
        SpxStrategy::new(self.mktdata.clone(), self.orders.clone())
    }

    async fn subscribe_to_updates(strategies: &[Strategy], mktdata: &Arc<RwLock<MktData>>) {
        fn get_underlying_instrument_type(instrument_type: OptionType) -> OptionType {
            match instrument_type {
                OptionType::EquityOption => OptionType::Equity,
                OptionType::FutureOption => OptionType::Future,
                _ => panic!("Unsupported Type"),
            }
        }

        async fn subscribe_to_symbol(
            symbol: &str,
            underlying: &str,
            event_type: &str,
            option_type: OptionType,
            strike_price: Option<Decimal>,
            mktdata: Arc<RwLock<MktData>>,
        ) {
            let mut write_lock = mktdata.write().await;
            if let Err(err) = write_lock
                .subscribe_to_feed(
                    symbol,
                    underlying,
                    vec![event_type].as_slice(),
                    option_type,
                    strike_price,
                )
                .await
            {
                error!(
                    "Failed to subscribe to symbol: {} feed, error: {}",
                    symbol, err
                );
            }
        }

        async fn subscribe_to_option_and_underlying<Strat>(
            strategy: &Strat,
            mktdata: &Arc<RwLock<MktData>>,
        ) where
            Strat: StrategyMeta + Sync + Send,
        {
            let underlying = strategy.get_underlying();
            for leg in strategy.get_position().legs.iter() {
                subscribe_to_symbol(
                    &leg.symbol,
                    underlying,
                    "Quote",
                    leg.option_type,
                    Some(leg.strike_price),
                    mktdata.clone(),
                )
                .await;
            }

            subscribe_to_symbol(
                underlying,
                underlying,
                "Quote",
                get_underlying_instrument_type(strategy.get_option_type()),
                None,
                mktdata.clone(),
            )
            .await;
        }

        for strategy in strategies {
            match &strategy {
                Strategy::Credit(strategy) => {
                    subscribe_to_option_and_underlying(strategy, mktdata).await
                }
                // Strategy::Calendar(strat) => subscribe(strat, mktdata).await,
                // Strategy::Condor(strat) => subscribe(strat, mktdata).await,
                _ => (),
            }
        }
    }

    async fn check_stops(
        strategy: &Strategy,
        mktdata: &Arc<RwLock<MktData>>,
        orders: &Arc<RwLock<Orders>>,
    ) -> Result<()> {
        async fn send_liquidate<Strat>(strat: &Strat, orders: Arc<RwLock<Orders>>) -> Result<()>
        where
            Strat: StrategyMeta,
        {
            let price_effect = match strat.get_position().legs[0].direction {
                Direction::Short => PriceEffect::Credit,
                Direction::Long => PriceEffect::Debit,
            };
            orders
                .write()
                .await
                .liquidate_position(strat, price_effect)
                .await
        }

        match strategy {
            Strategy::Credit(strat) => {
                if strat.should_exit(mktdata.clone()).await {
                    match send_liquidate(strat, orders.clone()).await {
                        Ok(val) => val,
                        Err(err) => error!("Failed to liquidate position, error: {}", err),
                    }
                }
            }
            // Strategy::Calendar(strat) => {
            //     if strat.should_exit(mktdata).await {
            //         match send_liquidate(strat, orders).await {
            //             Ok(val) => val,
            //             Err(err) => error!("Failed to liquidate position, error: {}", err),
            //         }
            //     }
            // }
            Strategy::Condor(strat) => {
                if strat.should_exit(mktdata.clone()).await {
                    match send_liquidate(strat, orders.clone()).await {
                        Ok(val) => val,
                        Err(err) => error!("Failed to liquidate position, error: {}", err),
                    }
                }
            }
            _ => (),
        }
        Ok(())
    }

    async fn get_strategies(web_client: &WebClient) -> Result<Vec<Strategy>> {
        let positions = match web_client
            .get::<AccountPositions>(
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

    async fn convert_api_data_into_strategies(legs: Vec<Leg>) -> Vec<Strategy> {
        let mut sorted_legs: HashMap<String, Vec<Leg>> = HashMap::new();

        legs.iter().for_each(|leg| {
            let underlying = leg.underlying_symbol.clone().unwrap(); // Assuming underlying_symbol is a string
            sorted_legs.entry(underlying).or_default().push(leg.clone());
        });

        let strats: Vec<Strategy> = sorted_legs
            .values()
            .map(|legs| {
                let spread = Position::new(legs.clone());

                match &spread.strategy_type {
                    StrategyType::CreditSpread => Strategy::Credit(CreditSpread::new(spread)),
                    StrategyType::CalendarSpread => Strategy::Calendar(CalendarSpread::new(spread)),
                    StrategyType::IronCondor => Strategy::Condor(IronCondor::new(spread)),
                    _ => Strategy::NotTracked,
                }
            })
            .collect();

        Self::print_strategy_data(&strats);
        strats
    }

    fn print_strategy_data(strats: &[Strategy]) {
        strats.iter().for_each(|strategy| match strategy {
            Strategy::Calendar(strat) => strat.print(),
            Strategy::Credit(strat) => strat.print(),
            Strategy::Condor(strat) => strat.print(),
            _ => (),
        });
    }
}
