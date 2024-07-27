use serde::Deserialize;
use serde::Serialize;

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
