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



// use crate::mktdata::tt_api::CandleData;
use crate::positions::tt_api as pos_api;
use crate::positions::ComplexSymbol;
use crate::positions::Direction;
use crate::positions::InstrumentType;

use crate::positions::StrategyType;

use super::account::Account;
use super::mktdata::MktData;
use super::orders::Orders;
use super::positions::OptionStrategy;
use super::web_client::WebClient;
// use super::orders::Orders;

trait StrategyMeta: Sync + Send {
    fn get_underlying(&self) -> &str;
    fn get_symbols(&self) -> Vec<&str>;
    fn get_instrument_type(&self) -> InstrumentType;
}

struct CreditSpread {
    position: OptionStrategy,
}

impl CreditSpread {
    fn new(position: OptionStrategy) -> Self {
        Self { position }
    }

    async fn should_exit(&self, mktdata: &MktData) -> anyhow::Result<bool> {
        for complex_symbol in &self.position.legs {
            if complex_symbol.direction().eq(&Direction::Short) {
                if let Some(snapshot) = mktdata.get_snapshot_data(complex_symbol.symbol()).await {
                    return Ok(snapshot.data.delta > 0.4);
                }
            }
        }
        anyhow::Ok(false)
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
}

struct CalendarSpread {
    position: OptionStrategy,
}

impl CalendarSpread {
    fn new(position: OptionStrategy) -> Self {
        Self { position }
    }

    async fn should_exit(&self, mktdata: &MktData) -> anyhow::Result<bool> {
        let mut total_theta = 0.;
        for complex_symbol in &self.position.legs {
            if let Some(snapshot) = mktdata.get_snapshot_data(complex_symbol.symbol()).await {
                total_theta += snapshot.data.theta;
            }
        }
        Ok(total_theta < 0.)
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
}

struct IronCondor {
    position: OptionStrategy,
}

impl IronCondor {
    fn new(position: OptionStrategy) -> Self {
        Self { position }
    }

    async fn should_exit(&self, mktdata: &MktData) -> anyhow::Result<bool> {
        for complex_symbol in &self.position.legs {
            if complex_symbol.direction().eq(&Direction::Short) {
                if let Some(snapshot) = mktdata.get_snapshot_data(complex_symbol.symbol()).await {
                    if snapshot.data.delta > 0.4 {
                        return anyhow::Ok(true);
                    }
                }
            }
        }
        anyhow::Ok(false)
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
        Ok(Self {})
    }

    async fn subscribe_to_updates(
        strategies: &[Strategy],
        mktdata: &mut MktData,
        _cancel_token: &CancellationToken,
    ) {
        async fn subscribe<Strat>(strategy: &Strat, mktdata: &mut MktData)
        where
            Strat: StrategyMeta,
        {
            if let Err(err) = mktdata
                .subscribe_to_mktdata(strategy.get_symbols(), strategy.get_instrument_type())
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
            orders.liquidate_position(&strat.get_symbols()).await
        }

        match strategy {
            Strategy::Credit(strat) => {
                if strat.should_exit(mktdata).await? {
                    return send_liquidate(strat, orders).await;
                }
            }
            Strategy::Calendar(strat) => {
                if strat.should_exit(mktdata).await? {
                    return send_liquidate(strat, orders).await;
                }
            }
            Strategy::Condor(strat) => {
                if strat.should_exit(mktdata).await? {
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
                let spread = OptionStrategy::new(legs.clone());

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
