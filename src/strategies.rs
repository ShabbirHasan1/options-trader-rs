use anyhow::bail;
use anyhow::Result;
use std::collections::HashMap;
use std::fmt;
use std::iter::Iterator;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::sync::RwLockWriteGuard;
use tokio::time::error::Elapsed;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::event;
use tracing::info;

// use crate::mktdata::tt_api::CandleData;
use super::account::Account;
use super::mktdata::MktData;
use super::orders::Orders;
use super::positions::Position;
use super::web_client::WebClient;
use crate::mktdata::tt_api::FeedEvent;
use crate::positions::tt_api as pos_api;
use crate::positions::ComplexSymbol;
use crate::positions::Direction;
use crate::positions::InstrumentType;
use crate::positions::OptionType;
use crate::positions::PriceEffect;
use crate::positions::StrategyType;
// use super::orders::Orders;

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
        fn get_back_leg_strike_price() {}

        fn get_back_leg_option_type(position: &Position) -> OptionType {
            position.legs[1].option_type()
        }

        let events = mktdata
            .get_snapshot_events(self.position.legs[1].symbol())
            .await;

        events
            .iter()
            .find(|event| match event {
                FeedEvent::QuoteEvent(_) => true,
                _ => false,
            })
            .and_then(|event| match event {
                FeedEvent::QuoteEvent(quote) => {
                    let strike_price = self.position.legs[1].strike_price();
                    let mid_price = (quote.ask_price + quote.bid_price) / 2.;
                    let result = match get_back_leg_option_type(&self.position) {
                        OptionType::Call => strike_price < mid_price,
                        OptionType::Put => strike_price > mid_price,
                    };
                    Some(result)
                }
                _ => None,
            })
            .unwrap()
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
        let mut total_theta = 0.;
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

    async fn should_exit(&self, mktdata: &MktData) -> bool {
        let front_leg_strikes: Vec<(String, f64, OptionType)> = self
            .position
            .legs
            .iter()
            .filter(|leg| leg.direction() == Direction::Short)
            .map(|leg| {
                (
                    leg.symbol().to_string(),
                    leg.strike_price(),
                    leg.option_type(),
                )
            })
            .collect();

        let mut result = false;
        for (symbol, strike_price, option_type) in &front_leg_strikes {
            let events = mktdata.get_snapshot_events(symbol).await;
            result |= events
                .iter()
                .find(|event| matches!(event, FeedEvent::QuoteEvent(_)))
                .and_then(|event| match event {
                    FeedEvent::QuoteEvent(quote) => {
                        let mid_price = (quote.bid_price + quote.ask_price) / 2.;
                        let result = match option_type {
                            OptionType::Put => *strike_price > mid_price,
                            OptionType::Call => *strike_price < mid_price,
                        };
                        Some(result)
                    }
                    _ => None,
                })
                .unwrap();
        }
        result
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
        let orders = Orders::new(
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
                            if let Err(err) = Self::check_stops(strategy, &read_guard, &orders).await {
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
            for symbol in strategy.get_symbols() {
                if let Err(err) = mktdata
                    .write()
                    .await
                    .subscribe_to_options_mktdata(symbol, symbol, strategy.get_instrument_type())
                    .await
                {
                    error!(
                        "Failed to subscribe to mktdata for symbol: {}, error: {}",
                        symbol, err
                    );
                }
            }
        }
        for strategy in strategies {
            match &strategy {
                Strategy::Credit(strat) => subscribe(strat, mktdata).await,
                Strategy::Calendar(strat) => subscribe(strat, mktdata).await,
                Strategy::Condor(strat) => subscribe(strat, mktdata).await,
                _ => (),
            }
        }
    }

    async fn check_stops(strategy: &Strategy, mktdata: &MktData, orders: &Orders) -> Result<()> {
        async fn send_liquidate<Strat>(strat: &Strat, orders: &Orders) -> Result<()>
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
                    return send_liquidate(strat, orders).await;
                }
            }
            Strategy::Calendar(strat) => {
                if strat.should_exit(mktdata).await {
                    return send_liquidate(strat, orders).await;
                }
            }
            Strategy::Condor(strat) => {
                if strat.should_exit(mktdata).await {
                    return send_liquidate(strat, orders).await;
                }
            }
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
