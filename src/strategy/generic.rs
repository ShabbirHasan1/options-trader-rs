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

use super::spx::SpxStrategy;
use crate::connectivity::web_client::WebClient;
use crate::platform::{account::Account, mktdata::*, orders::*, positions::*};
use crate::tt_api::{mktdata::Quote, positions::AccountPositions, positions::Leg};

pub(crate) trait StrategyMeta: Sync + Send {
    fn get_underlying(&self) -> &str;
    fn get_symbols(&self) -> Vec<&str>;
    fn get_option_type(&self) -> OptionType;
    fn get_position(&self) -> &Position;
}

pub(crate) struct CreditSpread {
    position: Position,
}

impl CreditSpread {
    pub fn new(position: Position) -> Self {
        Self { position }
    }

    pub async fn should_exit(&self, mktdata: Arc<RwLock<MktData>>) -> bool {
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

    fn get_option_type(&self) -> OptionType {
        self.position.legs.first().unwrap().option_type
    }

    fn get_position(&self) -> &Position {
        &self.position
    }
}

pub(crate) struct CalendarSpread {
    position: Position,
}

impl CalendarSpread {
    pub fn new(position: Position) -> Self {
        Self { position }
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

    fn get_option_type(&self) -> OptionType {
        self.position.legs.first().unwrap().option_type
    }

    fn get_position(&self) -> &Position {
        &self.position
    }
}

pub(crate) struct IronCondor {
    position: Position,
}

impl IronCondor {
    pub fn new(position: Position) -> Self {
        Self { position }
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

    fn get_option_type(&self) -> OptionType {
        self.position.legs.first().unwrap().option_type
    }

    fn get_position(&self) -> &Position {
        &self.position
    }
}
