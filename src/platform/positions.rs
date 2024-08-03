use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::fmt;
use std::str::FromStr;

use crate::tt_api::positions::*;

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
pub enum OptionSide {
    Call,
    Put,
    Neutral,
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

impl fmt::Display for OptionSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match &self {
                OptionSide::Call => "Call",
                OptionSide::Put => "Put",
                _ => panic!("Unknown option type"),
            }
        )
    }
}

impl OptionSide {
    pub fn parse(option_type: char) -> OptionSide {
        match option_type {
            'C' => OptionSide::Call,
            'P' => OptionSide::Put,
            _ => panic!("Unknown option type"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OptionType {
    Equity,
    EquityOption,
    Future,
    FutureOption,
}

impl fmt::Display for OptionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let instrument_type = match self {
            OptionType::Equity => String::from("Equity"),
            OptionType::Future => String::from("Future"),
            OptionType::EquityOption => String::from("EquityOption "),
            OptionType::FutureOption => String::from("FutureOption "),
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

impl OptionType {
    pub fn get_symbol_type(instrument_type: &str) -> OptionType {
        match instrument_type {
            "Equity" => OptionType::Equity,
            "Future" => OptionType::Future,
            "Equity Option" => OptionType::EquityOption,
            "Future Option" => OptionType::FutureOption,
            _ => panic!("Unsupported Type"),
        }
    }
}

fn parse_future_option(
    symbol: &str,
    underlying: &str,
    direction: &str,
    quantity: i32,
) -> Result<OptionLeg> {
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

    let expiration_date = match NaiveDate::parse_from_str(&parts[2][..6], "%y%m%d") {
        Ok(val) => val,
        Err(err) => bail!("Failed to parse date: {}, error: {}", parts[2], err),
    };
    let option_type = OptionSide::parse(parts[2].chars().nth(6).unwrap());

    //TODO fix the strike price
    let strike_price = Decimal::from_str(&parts[2][7..])?;

    Ok(OptionLeg {
        symbol: symbol.to_string(),
        underlying: underlying.to_string(),
        expiration_date,
        direction: Direction::parse(direction),
        side: option_type,
        strike_price,
        quantity,
        option_type: OptionType::FutureOption,
    })
}

fn parse_equity_option(
    symbol: &str,
    underlying: &str,
    direction: &str,
    quantity: i32,
) -> Result<OptionLeg> {
    if symbol.len() != 21 {
        bail!(
            "Invalid format whilst parsing equity option symbol: {}, len: {}",
            symbol,
            symbol.len()
        );
    }

    let expiration_date = match NaiveDate::parse_from_str(&symbol[6..12], "%y%m%d") {
        Ok(val) => val,
        Err(err) => bail!("Failed to parse date: {}, error: {}", &symbol[6..12], err),
    };
    let option_type = OptionSide::parse(symbol.chars().nth(12).unwrap());

    let strike_price = Decimal::from_str(symbol[13..].trim_start_matches('0'))?;
    let strike_price = strike_price / Decimal::new(1000, 0);

    Ok(OptionLeg {
        symbol: symbol.to_string(),
        underlying: underlying.to_string(),
        expiration_date,
        direction: Direction::parse(direction),
        side: option_type,
        strike_price,
        quantity,
        option_type: OptionType::EquityOption,
    })
}

#[derive(Debug)]
pub(crate) struct OptionLeg {
    pub symbol: String,
    pub underlying: String,
    pub expiration_date: NaiveDate,
    pub direction: Direction,
    pub side: OptionSide,
    pub strike_price: Decimal,
    pub quantity: i32,
    pub option_type: OptionType,
}

impl fmt::Display for OptionLeg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let fmt = format!(
            "symbol: {}, underlying: {}, expiration: {}, type:{}, strike: {}",
            self.symbol, self.underlying, self.expiration_date, self.side, self.strike_price
        );
        write!(f, "\nLeg {}", fmt)
    }
}

pub struct Position {
    pub legs: Vec<OptionLeg>,
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
    pub fn new(legs: Vec<Leg>) -> Position {
        let mut symbols = Self::parse_complex_symbols(&legs);
        let strategy_type = Self::determine_strategy(&symbols, &legs);
        symbols.sort_by(|a, b| {
            b.strike_price
                .partial_cmp(&a.strike_price)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Self {
            legs: symbols,
            strategy_type,
        }
    }

    fn parse_complex_symbols(legs: &[Leg]) -> Vec<OptionLeg> {
        fn unsupported_option_type(_: &str, _: &str, _: &str, _: i32) -> Result<OptionLeg> {
            Err(anyhow!("Unsupported option type"))
        }

        let symbols: Vec<OptionLeg> = legs
            .iter()
            .filter_map(|leg| {
                let parser = match OptionType::get_symbol_type(
                    leg.instrument_type.as_ref().unwrap().as_str(),
                ) {
                    OptionType::EquityOption => parse_equity_option,
                    OptionType::FutureOption => parse_future_option,
                    _ => unsupported_option_type,
                };
                parser(
                    &leg.symbol,
                    leg.underlying_symbol.as_ref().unwrap().as_str(),
                    leg.quantity_direction.as_ref().unwrap(),
                    leg.quantity,
                )
                .ok()
            })
            .collect();

        symbols
    }

    fn determine_strategy(symbols: &[OptionLeg], legs: &[Leg]) -> StrategyType {
        match legs.len() {
            1 => Self::single_leg_strategies(symbols),
            2 => Self::double_leg_strategies(symbols),
            4 => StrategyType::IronCondor,
            _ => StrategyType::Other,
        }
    }

    fn single_leg_strategies(symbols: &[OptionLeg]) -> StrategyType {
        match symbols[0].side {
            OptionSide::Call => StrategyType::Call,
            OptionSide::Put => StrategyType::Put,
            _ => panic!("No neutral option side"),
        }
    }

    fn double_leg_strategies(symbols: &[OptionLeg]) -> StrategyType {
        let leg1 = &symbols[0];
        let leg2 = &symbols[1];

        if leg1.expiration_date == leg2.expiration_date {
            return StrategyType::CreditSpread;
        }

        if leg1.strike_price == leg2.strike_price {
            return StrategyType::CalendarSpread;
        }
        StrategyType::Other
    }
}
