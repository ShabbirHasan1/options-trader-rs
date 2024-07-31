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
use crate::mktdata::Snapshot;
use crate::positions::Direction;
use crate::positions::OptionLeg;
use crate::positions::OptionSide;
use crate::positions::OptionType;
use crate::positions::PriceEffect;
use crate::positions::StrategyType;
use crate::tt_api::mktdata::Quote;
use crate::tt_api::positions::AccountPositions;
use crate::tt_api::positions::Leg;

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
    fn get_instrument_type(&self) -> OptionType;
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
        fn get_option_type(position: &Position) -> OptionSide {
            position.legs[0].side
        }

        fn get_strike_price(position: &Position) -> Decimal {
            match get_option_type(position) {
                OptionSide::Call => position.legs[1].strike_price,
                OptionSide::Put => position.legs[0].strike_price,
            }
        }

        fn get_midprice(snapshot: &Snapshot) -> Decimal {
            if let Some(quote) = &snapshot.quote {
                return quote.midprice();
            }
            dec!(0)
        }

        let mkt_event = mktdata
            .get_snapshot_by_symbol::<Quote>(self.get_underlying())
            .await;

        if let Some(snapshot) = mkt_event {
            let mid_price = get_midprice(&snapshot);
            if mid_price == dec!(0) {
                return false;
            }
            let strike_price = get_strike_price(&self.position);
            let result = match get_option_type(&self.position) {
                OptionSide::Call => strike_price < mid_price,
                OptionSide::Put => strike_price > mid_price,
            };

            info!(
                "Should exit position: {} mid price: {} has crossed strike price: {}",
                self.get_underlying(),
                mid_price,
                strike_price
            );
            result
        } else {
            false
        }
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
            &self.position.legs.first().unwrap().symbol,
            &self.position
        )
    }
}

impl StrategyMeta for CreditSpread {
    fn get_underlying(&self) -> &str {
        &self.position.legs.first().unwrap().underlying
    }

    fn get_symbols(&self) -> Vec<&str> {
        self.position
            .legs
            .iter()
            .map(|leg| leg.symbol.as_str())
            .collect()
    }

    fn get_instrument_type(&self) -> OptionType {
        self.position.legs.first().unwrap().option_type
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
            &self.position.legs.first().unwrap().symbol,
            &self.position
        )
    }
}

impl StrategyMeta for CalendarSpread {
    fn get_underlying(&self) -> &str {
        &self.position.legs.first().unwrap().underlying
    }

    fn get_symbols(&self) -> Vec<&str> {
        self.position
            .legs
            .iter()
            .map(|leg| leg.symbol.as_str())
            .collect()
    }

    fn get_instrument_type(&self) -> OptionType {
        self.position.legs.first().unwrap().option_type
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
        fn get_strike_prices(position: &Position) -> (Decimal, Decimal) {
            (position.legs[1].strike_price, position.legs[2].strike_price)
        }

        let mkt_event = mktdata
            .get_snapshot_by_symbol::<Quote>(self.get_underlying())
            .await;

        if let Some(snapshot) = mkt_event {
            let mid_price = snapshot.quote.unwrap().midprice();
            if mid_price == Decimal::default() {
                return false;
            }
            let (call_strike_price, put_strike_price) = get_strike_prices(&self.position);

            call_strike_price < mid_price || put_strike_price > mid_price
        } else {
            false
        }
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
            &self.position.legs.first().unwrap().underlying,
            &self.position
        )
    }
}

impl StrategyMeta for IronCondor {
    fn get_underlying(&self) -> &str {
        &self.position.legs.first().unwrap().underlying
    }

    fn get_symbols(&self) -> Vec<&str> {
        self.position
            .legs
            .iter()
            .map(|leg| leg.symbol.as_str())
            .collect()
    }

    fn get_instrument_type(&self) -> OptionType {
        self.position.legs.first().unwrap().option_type
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
                get_underlying_instrument_type(strategy.get_instrument_type()),
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
        mktdata: &MktData,
        orders: &mut Orders,
    ) -> Result<()> {
        async fn send_liquidate<Strat>(strat: &Strat, orders: &mut Orders) -> Result<()>
        where
            Strat: StrategyMeta,
        {
            let price_effect = match strat.get_position().legs[0].direction {
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
