use anyhow::bail;
use anyhow::Result;
use chrono::NaiveDate;
use serde::Deserialize;
use serde::Serialize;

mod tt_api {
    use super::*;

    #[derive(Debug, Deserialize, Serialize)]
    pub struct AccountPositions {
        pub data: Positions,
        pub context: String,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct Positions {
        #[serde(rename = "items")]
        pub Legs: Vec<Leg>,
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

#[derive(Debug, Clone, Copy)]
pub enum OptionType {
    Call,
    Put,
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

pub trait ComplexSymbol {
    fn symbol(&self) -> &str;
    fn expiration_date(&self) -> NaiveDate;
    fn option_type(&self) -> OptionType;
    fn strike_price(&self) -> i32;
}

#[derive(Debug)]
struct FutureOptionSymbol {
    future_contract_code: String,
    option_contract_code: String,
    expiration_date: NaiveDate,
    option_type: OptionType,
    strike_price: i32,
}

impl FutureOptionSymbol {
    pub fn parse(symbol: &str) -> Result<FutureOptionSymbol> {
        if symbol.len() < 20 || !symbol.starts_with("./") {
            bail!("Invalid format whilst parsing future option symbol");
        }

        let parts: Vec<&str> = symbol[2..].split_whitespace().collect();
        if parts.len() != 5 {
            bail!("Invalid number of tokens parsing future option symbol");
        }

        let future_contract_code = parts[0].to_string();
        let option_contract_code = parts[1].to_string();
        let expiration_date = match NaiveDate::parse_from_str(parts[2], "%Y%m%d") {
            Ok(val) => val,
            _ => bail!("Failed to parse date"),
        };
        let option_type = OptionType::parse(parts[3].chars().next().unwrap());

        let strike_price_str = parts[4].trim_start_matches('0');
        let strike_price = match strike_price_str.parse::<f64>() {
            Ok(strike) => (strike * 1000.0) as i32,
            Err(_) => bail!("Invalid strike price format"),
        };

        Ok(FutureOptionSymbol {
            future_contract_code,
            option_contract_code,
            expiration_date,
            option_type,
            strike_price,
        })
    }
}

impl ComplexSymbol for FutureOptionSymbol {
    fn symbol(&self) -> &str {
        &self.option_contract_code
    }

    fn expiration_date(&self) -> NaiveDate {
        self.expiration_date
    }

    fn option_type(&self) -> OptionType {
        self.option_type
    }

    fn strike_price(&self) -> i32 {
        self.strike_price
    }
}

#[derive(Debug)]
struct EquityOptionSymbol {
    symbol: String,
    expiration_date: NaiveDate,
    option_type: OptionType,
    strike_price: i32,
}

impl EquityOptionSymbol {
    pub fn parse(symbol: &str) -> Result<EquityOptionSymbol> {
        if symbol.len() != 20 {
            bail!("Invalid format whilst parsing equity option symbol");
        }

        let symbol = symbol[0..6].trim().to_string();
        let expiration_date = match NaiveDate::parse_from_str(&symbol[6..12], "%Y%m%d") {
            Ok(val) => val,
            _ => bail!("Failed to parse date"),
        };
        let option_type = OptionType::parse(symbol.chars().nth(12).unwrap());

        let strike_price_str = symbol[13..].trim_start_matches('0');
        let strike_price = match strike_price_str.parse::<f64>() {
            Ok(strike) => (strike * 1000.0) as i32,
            Err(_) => bail!("Invalid strike price format"),
        };

        Ok(EquityOptionSymbol {
            symbol,
            expiration_date,
            option_type,
            strike_price,
        })
    }
}

impl ComplexSymbol for EquityOptionSymbol {
    fn symbol(&self) -> &str {
        &self.symbol
    }

    fn expiration_date(&self) -> NaiveDate {
        self.expiration_date
    }

    fn option_type(&self) -> OptionType {
        self.option_type
    }

    fn strike_price(&self) -> i32 {
        self.strike_price
    }
}

#[derive(Debug, Clone, Copy)]
pub enum InstrumentType {
    Equity,
    Future,
}

impl InstrumentType {
    pub fn get_symbol_type(instrument_type: &str) -> InstrumentType {
        match instrument_type {
            "Equity Option" => InstrumentType::Equity,
            "Future Option" => InstrumentType::Future,
            _ => panic!("Unsupported Type"),
        }
    }
}

pub struct OptionStrategy<ST> {
    symbols: Vec<ST>,
    strategy_type: StrategyType,
    legs: Vec<tt_api::Leg>,
}

impl<ST> OptionStrategy<ST> {
    pub fn new(symbols: Vec<ST>, legs: Vec<tt_api::Leg>) -> OptionStrategy<ST>
    where
        ST: ComplexSymbol,
    {
        let strategy_type = Self::determine_strategy(&symbols, &legs);
        Self {
            symbols,
            strategy_type,
            legs,
        }
    }

    fn determine_strategy(symbols: &Vec<ST>, legs: &Vec<tt_api::Leg>) -> StrategyType
    where
        ST: ComplexSymbol,
    {
        match legs.len() {
            1 => Self::single_leg_strategies(symbols),
            2 => Self::double_leg_strategies(symbols),
            _ => StrategyType::Other,
        }
    }

    fn single_leg_strategies(symbols: &Vec<ST>) -> StrategyType
    where
        ST: ComplexSymbol,
    {
        match symbols[0].option_type() {
            OptionType::Call => StrategyType::Call,
            OptionType::Put => StrategyType::Put,
        }
    }

    fn double_leg_strategies(symbols: &Vec<ST>) -> StrategyType
    where
        ST: ComplexSymbol,
    {
        if symbols[0].expiration_date() == symbols[1].expiration_date() {
            return StrategyType::CreditSpread;
        }

        if symbols[0].strike_price() == symbols[1].strike_price() {
            return StrategyType::CalendarSpread;
        }

        StrategyType::Other
    }
}
