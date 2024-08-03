use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::fmt;
use std::iter::Iterator;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::platform::{mktdata::*, positions::*};
use crate::tt_api::mktdata::Quote;

fn get_mid_price(snapshot: &Snapshot) -> Decimal {
    if let Some(quote) = &snapshot.quote {
        return quote.midprice();
    }
    Decimal::default()
}

pub enum Direction {
    Bullish,
    Bearish,
    Neutral,
}

pub(crate) trait StrategyMeta: Sync + Send {
    fn get_underlying(&self) -> &str;
    fn get_symbols(&self) -> Vec<&str>;
    fn get_direction(&self) -> Direction;
    fn get_price_effect(&self) -> PriceEffect;
    fn get_type(&self) -> OptionType;
    fn get_position(&self) -> &Position;
}

pub(crate) struct CreditSpread {
    position: Position,
}

impl CreditSpread {
    pub fn new(position: Position) -> Self {
        Self { position }
    }

    pub async fn calculate_mid_price(
        underlying: &str,
        direction: Direction,
        mktdata: Arc<RwLock<MktData>>,
    ) -> Decimal {
        let reader = mktdata.read().await;
        let snapshots = reader
            .group_snapshots_by_underlying::<Quote>(underlying)
            .await;
        let (short_option_mid, long_option_mid) = match direction {
            Direction::Bearish => (get_mid_price(&snapshots[1]), get_mid_price(&snapshots[0])),
            Direction::Bullish => (get_mid_price(&snapshots[0]), get_mid_price(&snapshots[1])),
            _ => panic!("No neutral option side on a credit spread"),
        };

        let mid_price = short_option_mid - long_option_mid;

        info!(
            "New calc symbol:{} mid: {} (sold: {}, bought: {})",
            underlying, mid_price, short_option_mid, long_option_mid
        );
        mid_price
    }

    pub async fn should_exit(&self, mktdata: Arc<RwLock<MktData>>) -> bool {
        fn get_option_type(position: &Position) -> OptionSide {
            position.legs[0].side
        }

        fn get_strike_price(position: &Position) -> Decimal {
            match get_option_type(position) {
                OptionSide::Call => position.legs[1].strike_price,
                OptionSide::Put => position.legs[0].strike_price,
                _ => panic!("No neutral option side on a credit spread"),
            }
        }

        fn get_midprice(snapshot: &Snapshot) -> Decimal {
            if let Some(quote) = &snapshot.quote {
                return quote.midprice();
            }
            dec!(0)
        }

        let mkt_event = mktdata
            .read()
            .await
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
                _ => panic!("No neutral option side on a credit spread"),
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

    pub fn print(&self) {
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

    fn get_type(&self) -> OptionType {
        self.position.legs.first().unwrap().option_type
    }

    fn get_position(&self) -> &Position {
        &self.position
    }

    fn get_direction(&self) -> Direction {
        match self.position.legs[0].side {
            OptionSide::Call => Direction::Bearish,
            OptionSide::Put => Direction::Bullish,
            _ => panic!("No neutral option side on a credit spread"),
        }
    }
    fn get_price_effect(&self) -> PriceEffect {
        PriceEffect::Credit
    }
}

pub(crate) struct CalendarSpread {
    position: Position,
}

impl CalendarSpread {
    pub fn new(position: Position) -> Self {
        Self { position }
    }

    async fn calculate_mid_price(&self, _mktdata: Arc<RwLock<MktData>>) -> Decimal {
        dec!(0)
    }

    pub async fn should_exit(&self, mktdata: &MktData) -> bool {
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

    pub fn print(&self) {
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

    fn get_type(&self) -> OptionType {
        self.position.legs.first().unwrap().option_type
    }

    fn get_position(&self) -> &Position {
        &self.position
    }

    fn get_direction(&self) -> Direction {
        Direction::Neutral
    }

    fn get_price_effect(&self) -> PriceEffect {
        PriceEffect::Credit
    }
}

pub(crate) struct IronCondor {
    position: Position,
}

impl IronCondor {
    pub fn new(position: Position) -> Self {
        Self { position }
    }

    pub async fn calculate_mid_price(underlying: &str, mktdata: Arc<RwLock<MktData>>) -> Decimal {
        let reader = mktdata.read().await;
        let snapshots = reader
            .group_snapshots_by_underlying::<Quote>(underlying)
            .await;

        let long_call_mid = get_mid_price(&snapshots[0]);
        let short_call_mid = get_mid_price(&snapshots[1]);
        let short_put_mid = get_mid_price(&snapshots[2]);
        let long_put_mid = get_mid_price(&snapshots[3]);

        let mid_price = (short_call_mid - long_call_mid) + (short_put_mid - long_put_mid);

        info!(
            "New calc mid: {} call spread: {} put spread: {}",
            mid_price,
            short_call_mid - long_call_mid,
            short_put_mid - long_put_mid,
        );
        mid_price
    }

    //Matches the near leg strike price against underlying mid price
    pub async fn should_exit(&self, mktdata: Arc<RwLock<MktData>>) -> bool {
        fn get_strike_prices(position: &Position) -> (Decimal, Decimal) {
            (position.legs[1].strike_price, position.legs[2].strike_price)
        }

        let mkt_event = mktdata
            .read()
            .await
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

    pub fn print(&self) {
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

    fn get_type(&self) -> OptionType {
        self.position.legs.first().unwrap().option_type
    }

    fn get_position(&self) -> &Position {
        &self.position
    }

    fn get_direction(&self) -> Direction {
        Direction::Neutral
    }

    fn get_price_effect(&self) -> PriceEffect {
        PriceEffect::Credit
    }
}
