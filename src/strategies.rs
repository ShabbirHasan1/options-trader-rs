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

// use crate::mktdata::tt_api::CandleData;
use super::account::Account;
use super::mktdata::MktData;
use super::orders::Orders;
use super::positions::Position;
use super::web_client::WebClient;
use crate::positions::tt_api as pos_api;
use crate::positions::ComplexSymbol;
use crate::positions::Direction;
use crate::positions::InstrumentType;
use crate::positions::OptionType;
use crate::positions::PriceEffect;
use crate::positions::StrategyType;
use crate::tt_api::mkt_data::FeedEvent;
use crate::tt_api::mkt_data::Quote;

struct SpxSpread {
    web_client: Arc<RwLock<WebClient>>,
}

impl SpxSpread {
    fn new(web_client: Arc<RwLock<WebClient>>) -> Self {
        Self::market_monitor();
        Self { web_client }
    }

    fn market_monitor() {
        tokio::spawn(async move {
            tokio::select! {
                _ = sleep(Duration::from_secs(5)) => {
                    // if orders.has_symbol() {
                    //     return
                    // }

                    // let diretion = get_market_conditions();
                    // if diretion.is_none() {
                    //     return
                    // }
                    // let _ = orders.enter_position();
                }
            }
        });
    }
}

pub(crate) trait StrategyMeta: Sync + Send {
    fn get_underlying(&self) -> &str;
    fn get_symbols(&self) -> Vec<&str>;
    fn get_instrument_type(&self) -> InstrumentType;
    fn get_position(&self) -> &Position;
}

struct CreditSpread {
    position: Position,
}

impl CreditSpread {
    fn new(position: Position) -> Self {
        Self { position }
    }

    async fn should_exit(&self, mktdata: &MktData) -> bool {
        fn get_option_type(position: &Position) -> OptionType {
            position.legs[1].option_type()
        }

        let mkt_event = mktdata
            .get_snapshot_by_symbol::<Quote>(match get_option_type(&self.position) {
                OptionType::Call => self.position.legs[1].symbol(),
                OptionType::Put => self.position.legs[0].symbol(),
            })
            .await;

        let strike_price = match get_option_type(&self.position) {
            OptionType::Call => self.position.legs[1].strike_price(),
            OptionType::Put => self.position.legs[0].strike_price(),
        };

        let mut result = false;
        if let Some(mkt_event) = mkt_event {
            result = match get_option_type(&self.position) {
                OptionType::Call => strike_price < mkt_event.midprice(),
                OptionType::Put => strike_price > mkt_event.midprice(),
            };

            info!(
                "Should exit position: {} mid price: {} has crossed strike price: {}",
                self.get_underlying(),
                mkt_event.midprice(),
                strike_price
            );
        }
        result
    }

    fn print(&self) {
        info!("{}", &self);
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

impl StrategyMeta for CreditSpread {
    fn get_underlying(&self) -> &str {
        self.position.legs.first().unwrap().underlying()
    }

    fn get_symbols(&self) -> Vec<&str> {
        self.position.legs.iter().map(|leg| leg.symbol()).collect()
    }

    fn get_instrument_type(&self) -> InstrumentType {
        self.position.legs.first().unwrap().instrument_type()
    }

    fn get_position(&self) -> &Position {
        &self.position
    }
}

struct CalendarSpread {
    position: Position,
}

impl CalendarSpread {
    fn new(position: Position) -> Self {
        Self { position }
    }

    async fn should_exit(&self, mktdata: &MktData) -> bool {
        let total_theta = 0.;
        for complex_symbol in &self.position.legs {
            // if let Some(event) = mktdata.get_snapshot_events(complex_symbol.symbol()).await {
            //     // if let FeedEvent::GreeksEvent(greek) = event {
            //     //     total_theta += greek.theta;
            //     // }
            // }
        }
        total_theta < 0.
    }

    fn print(&self) {
        info!("{}", &self);
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

impl StrategyMeta for CalendarSpread {
    fn get_underlying(&self) -> &str {
        self.position.legs.first().unwrap().underlying()
    }

    fn get_symbols(&self) -> Vec<&str> {
        self.position.legs.iter().map(|leg| leg.symbol()).collect()
    }

    fn get_instrument_type(&self) -> InstrumentType {
        self.position.legs.first().unwrap().instrument_type()
    }

    fn get_position(&self) -> &Position {
        &self.position
    }
}

struct IronCondor {
    position: Position,
}

impl IronCondor {
    fn new(position: Position) -> Self {
        Self { position }
    }

    //Matches the near leg strike price against underlying mid price
    async fn should_exit(&self, mktdata: &MktData) -> bool {
        fn get_ic_front_legs_strike_prices(position: &Position) -> (Decimal, Decimal) {
            (
                position.legs[1].strike_price(),
                position.legs[2].strike_price(),
            )
        }

        let call_front_leg = mktdata
            .get_snapshot_by_symbol::<Quote>(self.get_position().legs[1].symbol())
            .await;
        let put_front_leg = mktdata
            .get_snapshot_by_symbol::<Quote>(self.get_position().legs[2].symbol())
            .await;

        if call_front_leg.is_none() || put_front_leg.is_none() {
            return false;
        }

        let (call_strike_price, put_strike_price) = get_ic_front_legs_strike_prices(&self.position);
        call_strike_price < call_front_leg.unwrap().midprice()
            && put_strike_price > put_front_leg.unwrap().midprice()
    }

    fn print(&self) {
        info!("{}", &self);
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

impl StrategyMeta for IronCondor {
    fn get_underlying(&self) -> &str {
        self.position.legs.first().unwrap().underlying()
    }

    fn get_symbols(&self) -> Vec<&str> {
        self.position.legs.iter().map(|leg| leg.symbol()).collect()
    }

    fn get_instrument_type(&self) -> InstrumentType {
        self.position.legs.first().unwrap().instrument_type()
    }

    fn get_position(&self) -> &Position {
        &self.position
    }
}

enum Strategy {
    Calendar(CalendarSpread),
    Credit(CreditSpread),
    Condor(IronCondor),
    NotTracked,
}

pub(crate) struct Strategies {}

impl Strategies {
    pub async fn new(web_client: Arc<WebClient>, cancel_token: CancellationToken) -> Result<Self> {
        let _account = Account::new(Arc::clone(&web_client), cancel_token.clone());
        let mktdata = Arc::new(RwLock::new(MktData::new(
            Arc::clone(&web_client),
            cancel_token.clone(),
        )));
        let mut orders = Orders::new(
            Arc::clone(&web_client),
            Arc::clone(&mktdata),
            cancel_token.clone(),
        );
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
                        let read_guard = mktdata.read().await;
                        for strategy in &strategies {
                            if let Err(err) = Self::check_stops(strategy, &read_guard, &mut orders).await {
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
        Ok(Self {})
    }

    async fn subscribe_to_updates(
        strategies: &[Strategy],
        mktdata: &Arc<RwLock<MktData>>,
        _cancel_token: &CancellationToken,
    ) {
        async fn subscribe<Strat>(strategy: &Strat, mktdata: &Arc<RwLock<MktData>>)
        where
            Strat: StrategyMeta + Sync + Send,
        {
            fn get_underlying_instrument_type(instrument_type: InstrumentType) -> InstrumentType {
                match instrument_type {
                    InstrumentType::EquityOption => InstrumentType::Equity,
                    InstrumentType::FutureOption => InstrumentType::Future,
                    _ => panic!("Unsupported Type"),
                }
            }

            let mut write_lock = mktdata.write().await;

            for leg in strategy.get_position().legs.iter() {
                let result = write_lock
                    .subscribe_to_feed(
                        leg.symbol(),
                        strategy.get_underlying(),
                        vec!["Quote"].as_slice(),
                        strategy.get_instrument_type(),
                        Some(leg.strike_price()),
                    )
                    .await;

                if let Err(err) = result {
                    error!("Failed to subscribe to feed, error: {}", err);
                }
            }
            write_lock
                .subscribe_to_feed(
                    strategy.get_underlying(),
                    strategy.get_underlying(),
                    vec!["Quote"].as_slice(),
                    get_underlying_instrument_type(strategy.get_instrument_type()),
                    None,
                )
                .await;

            // for symbol in strategy.get_symbols() {
            //     if let Err(err) = mktdata
            //         .write()
            //         .await
            //         .subscribe_to_feed(
            //             symbol,
            //             strategy.get_underlying(),
            // vec!["Quote", "Greeks"].as_slice(),
            //             strategy.get_instrument_type(),
            //         )
            //         .await
            //     {
            //         error!(
            //             "Failed to subscribe to mktdata for symbol: {}, error: {}",
            //             symbol, err
            //         );
            //     }
            // }
        }
        for strategy in strategies {
            match &strategy {
                Strategy::Credit(strategy) => subscribe(strategy, mktdata).await,
                // Strategy::Calendar(strat) => subscribe(strat, mktdata).await,
                // Strategy::Condor(strat) => subscribe(strat, mktdata).await,
                _ => (),
            }
        }
    }

    async fn check_stops(
        strategy: &Strategy,
        mktdata: &MktData,
        orders: &mut Orders,
    ) -> Result<()> {
        async fn send_liquidate<Strat>(strat: &Strat, orders: &mut Orders) -> Result<()>
        where
            Strat: StrategyMeta,
        {
            let price_effect = match strat.get_position().legs[0].direction() {
                Direction::Short => PriceEffect::Credit,
                Direction::Long => PriceEffect::Debit,
            };
            orders.liquidate_position(strat, price_effect).await
        }

        match strategy {
            Strategy::Credit(strat) => {
                if strat.should_exit(mktdata).await {
                    match send_liquidate(strat, orders).await {
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
            // Strategy::Condor(strat) => {
            //     if strat.should_exit(mktdata).await {
            //         match send_liquidate(strat, orders).await {
            //             Ok(val) => val,
            //             Err(err) => error!("Failed to liquidate position, error: {}", err),
            //         }
            //     }
            // }
            _ => (),
        }
        Ok(())
    }

    async fn get_strategies(web_client: &WebClient) -> Result<Vec<Strategy>> {
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

    async fn convert_api_data_into_strategies(legs: Vec<pos_api::Leg>) -> Vec<Strategy> {
        let mut sorted_legs: HashMap<String, Vec<pos_api::Leg>> = HashMap::new();

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
