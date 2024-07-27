use rust_decimal::Decimal;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LegData {
    pub instrument_type: String,
    pub symbol: String,
    pub quantity: i32,
    pub remaining_quantity: i32,
    pub action: String,
    pub fills: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OrderData {
    pub id: i32,
    pub account_number: String,
    pub time_in_force: String,
    pub order_type: String,
    pub size: i32,
    pub underlying_symbol: String,
    pub underlying_instrument_type: String,
    pub status: String,
    pub cancellable: bool,
    pub editable: bool,
    pub edited: bool,
    pub legs: Vec<LegData>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Order {
    pub time_in_force: String,
    pub order_type: String,
    // pub stop_trigger: Option<u32>,
    #[serde(with = "rust_decimal::serde::float")]
    pub price: Decimal,
    pub price_effect: String,
    // pub value: Option<u32>,
    // pub value_effect: Option<String>,
    // pub gtc_date: Option<String>,
    // pub source: Option<String>,
    // pub partition_key: Option<String>,
    // pub preflight_id: Option<String>,
    // pub automated_source: Option<bool>,
    pub legs: Vec<Leg>,
    // pub rules: Option<Rules>,
    // pub advanced_instructions: Option<AdvancedInstructions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Leg {
    pub instrument_type: String,
    pub symbol: String,
    pub quantity: i32,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Rules {
    pub route_after: String,
    pub cancel_at: String,
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Condition {
    pub action: String,
    pub symbol: String,
    pub instrument_type: String,
    pub indicator: String,
    pub comparator: String,
    pub threshold: u32,
    pub price_components: Vec<PriceComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PriceComponent {
    pub symbol: String,
    pub instrument_type: String,
    pub quantity: u32,
    pub quantity_direction: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AdvancedInstructions {
    pub strict_position_effect_validation: bool,
}
