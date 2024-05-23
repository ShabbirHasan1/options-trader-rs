use anyhow::bail;
use anyhow::Result;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Deserialize;
use serde::Serialize;
use std::fmt;
use std::str::FromStr;
use tracing::debug;
use tracing::info;

pub(crate) mod tt_api {
    use super::*;

    #[derive(Debug, Deserialize, Serialize)]
    pub struct AccountPositions {
        pub data: Positions,
        pub context: String,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct Positions {
        #[serde(rename = "items")]
        pub legs: Vec<Leg>,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct Leg {
        #[serde(rename = "instrument-type")]
        pub instrument_type: Option<String>,
        pub multiplier: Option<i32>,
        #[serde(rename = "realized-today")]
        pub realized_today: Option<String>,
        #[serde(rename = "is-frozen")]
        pub is_frozen: bool,
        #[serde(rename = "updated-at")]
        pub updated_at: Option<String>,
        #[serde(rename = "average-daily-market-close-price")]
        pub average_daily_market_close_price: Option<String>,
        #[serde(rename = "deliverable-type")]
        pub deliverable_type: Option<String>,
        #[serde(rename = "underlying-symbol")]
        pub underlying_symbol: Option<String>,
        #[serde(rename = "mark-price")]
        pub mark_price: Option<String>,
        #[serde(rename = "account-number")]
        pub account_number: Option<String>,
        #[serde(rename = "fixing-price")]
        pub fixing_price: Option<String>,
        pub quantity: i32,
        #[serde(rename = "realized-day-gain-date")]
        pub realized_day_gain_date: Option<String>,
        #[serde(rename = "expires-at")]
        pub expires_at: Option<String>,
        pub mark: Option<Option<String>>,
        #[serde(rename = "realized-day-gain")]
        pub realized_day_gain: Option<String>,
        #[serde(rename = "realized-day-gain-effect")]
        pub realized_day_gain_effect: Option<String>,
        #[serde(rename = "cost-effect")]
        pub cost_effect: Option<String>,
        #[serde(rename = "close-price")]
        pub close_price: Option<String>,
        #[serde(rename = "average-yearly-market-close-price")]
        pub average_yearly_market_close_price: Option<String>,
        #[serde(rename = "average-open-price")]
        pub average_open_price: Option<String>,
        #[serde(rename = "is-suppressed")]
        pub is_suppressed: bool,
        pub created_at: Option<String>,
        pub symbol: String,
        #[serde(rename = "realized-today-date")]
        pub realized_today_date: Option<String>,
        #[serde(rename = "order-id")]
        pub order_id: Option<String>,
        #[serde(rename = "realized-today-effect")]
        pub realized_today_effect: Option<String>,
        #[serde(rename = "quantity-direction")]
        pub quantity_direction: Option<String>,
        #[serde(rename = "restricted-quantity")]
        pub restricted_quantity: Option<i32>,
    }
}

#[derive(Debug, Clone, Copy)]
pub enum StrategyType {
    Call,
    Put,
    CreditSpread,
    IronCondor,
    CalendarSpread,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OptionType {
    Call,
    Put,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Direction {
    Long,
    Short,
}

impl Direction {
    pub fn parse(direction: &str) -> Direction {
        match direction {
            "Long" => Direction::Long,
            "Short" => Direction::Short,
            _ => panic!("Unknown option type"),
        }
    }
}

impl fmt::Display for OptionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match &self {
                OptionType::Call => "Call",
                OptionType::Put => "Put",
                _ => panic!("Unknown option type"),
            }
        )
    }
}

impl OptionType {
    pub fn parse(option_type: char) -> OptionType {
        match option_type {
            'C' => OptionType::Call,
            'P' => OptionType::Put,
            _ => panic!("Unknown option type"),
        }
    }
}

pub trait ComplexSymbol: Send + Sync {
    fn symbol(&self) -> &str;
    fn direction(&self) -> Direction;
    fn underlying(&self) -> &str;
    fn expiration_date(&self) -> NaiveDate;
    fn option_type(&self) -> OptionType;
    fn instrument_type(&self) -> InstrumentType;
    fn strike_price(&self) -> Decimal;
    fn quantity(&self) -> i32;
    fn print(&self) -> String;
}

impl fmt::Display for dyn ComplexSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\nLeg {}", self.print())
    }
}

#[derive(Debug)]
struct FutureOptionSymbol {
    symbol: String,
    underlying: String,
    expiration_date: NaiveDate,
    direction: Direction,
    option_type: OptionType,
    strike_price: Decimal,
    quantity: i32,
}

impl FutureOptionSymbol {
    pub fn parse(symbol: &str, direction: &str, quantity: i32) -> Result<Box<dyn ComplexSymbol>> {
        info!("Futures symbol: {}", symbol);
        if symbol.len() < 20 || !symbol.starts_with("./") {
            bail!(
                "Invalid format whilst parsing future option symbol: {} len: {}",
                symbol,
                symbol.len()
            );
        }

        let parts: Vec<&str> = symbol[1..].split_whitespace().collect();
        if parts.len() != 3 {
            bail!(
                "Invalid number of tokens parsing future option symbol: {} tokens: {}",
                symbol,
                parts.len()
            );
        }

        let underlying = parts[0].to_string();
        let expiration_date = match NaiveDate::parse_from_str(&parts[2][..6], "%y%m%d") {
            Ok(val) => val,
            Err(err) => bail!("Failed to parse date: {}, error: {}", parts[2], err),
        };
        let option_type = OptionType::parse(parts[2].chars().nth(6).unwrap());

        //TODO fix the strike price
        let strike_price = Decimal::from_str(&parts[2][7..])?;

        Ok(Box::new(FutureOptionSymbol {
            symbol: symbol.to_string(),
            underlying: underlying.to_string(),
            expiration_date,
            direction: Direction::parse(direction),
            option_type,
            strike_price,
            quantity,
        }))
    }
}

impl ComplexSymbol for FutureOptionSymbol {
    fn print(&self) -> String {
        format!(
            "symbol: {}, underlying: {}, expiration: {}, type:{}, strike: {}",
            self.symbol, self.underlying, self.expiration_date, self.option_type, self.strike_price
        )
    }

    fn symbol(&self) -> &str {
        &self.symbol
    }

    fn underlying(&self) -> &str {
        &self.underlying
    }

    fn expiration_date(&self) -> NaiveDate {
        self.expiration_date
    }

    fn direction(&self) -> Direction {
        self.direction
    }

    fn option_type(&self) -> OptionType {
        self.option_type
    }

    fn instrument_type(&self) -> InstrumentType {
        InstrumentType::FutureOption
    }

    fn quantity(&self) -> i32 {
        self.quantity
    }

    fn strike_price(&self) -> Decimal {
        self.strike_price
    }
}

#[derive(Debug)]
struct EquityOptionSymbol {
    symbol: String,
    underlying: String,
    expiration_date: NaiveDate,
    direction: Direction,
    option_type: OptionType,
    strike_price: Decimal,
    quantity: i32,
}

impl EquityOptionSymbol {
    pub fn parse(symbol: &str, direction: &str, quantity: i32) -> Result<Box<dyn ComplexSymbol>> {
        if symbol.len() != 21 {
            bail!(
                "Invalid format whilst parsing equity option symbol: {}, len: {}",
                symbol,
                symbol.len()
            );
        }

        let underlying = symbol[0..6].trim().to_string();
        let expiration_date = match NaiveDate::parse_from_str(&symbol[6..12], "%y%m%d") {
            Ok(val) => val,
            Err(err) => bail!("Failed to parse date: {}, error: {}", &symbol[6..12], err),
        };
        let option_type = OptionType::parse(symbol.chars().nth(12).unwrap());

        let strike_price = Decimal::from_str(symbol[13..].trim_start_matches('0'))?;
        let strike_price = strike_price / Decimal::new(1000, 0);

        Ok(Box::new(EquityOptionSymbol {
            symbol: symbol.to_string(),
            underlying,
            expiration_date,
            direction: Direction::parse(direction),
            option_type,
            strike_price,
            quantity,
        }))
    }
}

impl ComplexSymbol for EquityOptionSymbol {
    fn print(&self) -> String {
        format!(
            "symbol: {}, underlying: {}, expiration: {}, type:{}, strike: {}",
            self.symbol, self.underlying, self.expiration_date, self.option_type, self.strike_price
        )
    }

    fn symbol(&self) -> &str {
        &self.symbol
    }

    fn underlying(&self) -> &str {
        &self.underlying
    }

    fn expiration_date(&self) -> NaiveDate {
        self.expiration_date
    }

    fn direction(&self) -> Direction {
        self.direction
    }

    fn option_type(&self) -> OptionType {
        self.option_type
    }

    fn instrument_type(&self) -> InstrumentType {
        InstrumentType::EquityOption
    }

    fn quantity(&self) -> i32 {
        self.quantity
    }

    fn strike_price(&self) -> Decimal {
        self.strike_price
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InstrumentType {
    Equity,
    EquityOption,
    Future,
    FutureOption,
}

impl fmt::Display for InstrumentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let instrument_type = match self {
            InstrumentType::Equity => String::from("Equity"),
            InstrumentType::Future => String::from("Future"),
            InstrumentType::EquityOption => String::from("EquityOption "),
            InstrumentType::FutureOption => String::from("FutureOption "),
        };
        write!(f, "{}", instrument_type)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PriceEffect {
    Credit,
    Debit,
}

impl fmt::Display for PriceEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let price_effect = match self {
            PriceEffect::Credit => String::from("Credit"),
            PriceEffect::Debit => String::from("Debit"),
        };
        write!(f, "{}", price_effect)
    }
}

impl InstrumentType {
    pub fn get_symbol_type(instrument_type: &str) -> InstrumentType {
        match instrument_type {
            "Equity" => InstrumentType::Equity,
            "Future" => InstrumentType::Future,
            "Equity Option" => InstrumentType::EquityOption,
            "Future Option" => InstrumentType::FutureOption,
            _ => panic!("Unsupported Type"),
        }
    }
}

pub struct Position {
    pub legs: Vec<Box<dyn ComplexSymbol>>,
    pub strategy_type: StrategyType,
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let leg_strings: Vec<String> = self.legs.iter().map(|leg| format!("{}", leg)).collect();
        let concatenated_legs = leg_strings.join(", ");
        write!(f, "{}", concatenated_legs)
    }
}

impl Position {
    pub fn new(legs: Vec<tt_api::Leg>) -> Position {
        let symbols = Self::parse_complex_symbols(&legs);
        let strategy_type = Self::determine_strategy(&symbols, &legs);
        Self {
            legs: symbols,
            strategy_type,
        }
    }

    fn parse_complex_symbols(legs: &[tt_api::Leg]) -> Vec<Box<dyn ComplexSymbol>> {
        let symbols: Vec<Box<dyn ComplexSymbol>> = legs
            .iter()
            .filter_map(|leg| {
                match InstrumentType::get_symbol_type(
                    leg.instrument_type.as_ref().unwrap().as_str(),
                ) {
                    InstrumentType::EquityOption => Some(
                        EquityOptionSymbol::parse(
                            &leg.symbol,
                            leg.quantity_direction.as_ref().unwrap(),
                            leg.quantity,
                        )
                        .unwrap(),
                    ),
                    InstrumentType::FutureOption => Some(
                        FutureOptionSymbol::parse(
                            &leg.symbol,
                            leg.quantity_direction.as_ref().unwrap(),
                            leg.quantity,
                        )
                        .unwrap(),
                    ),
                    InstrumentType::Future | InstrumentType::Equity => None,
                }
            })
            .collect();

        symbols
    }

    fn determine_strategy(
        symbols: &[Box<dyn ComplexSymbol>],
        legs: &[tt_api::Leg],
    ) -> StrategyType {
        match legs.len() {
            1 => Self::single_leg_strategies(symbols),
            2 => Self::double_leg_strategies(symbols),
            4 => StrategyType::IronCondor,
            _ => StrategyType::Other,
        }
    }

    fn single_leg_strategies(symbols: &[Box<dyn ComplexSymbol>]) -> StrategyType {
        match symbols[0].option_type() {
            OptionType::Call => StrategyType::Call,
            OptionType::Put => StrategyType::Put,
        }
    }

    fn double_leg_strategies(symbols: &[Box<dyn ComplexSymbol>]) -> StrategyType {
        let leg1 = &symbols[0];
        let leg2 = &symbols[1];

        if leg1.expiration_date() == leg2.expiration_date() {
            return StrategyType::CreditSpread;
        }

        if leg1.strike_price() == leg2.strike_price() {
            return StrategyType::CalendarSpread;
        }
        StrategyType::Other
    }
}
