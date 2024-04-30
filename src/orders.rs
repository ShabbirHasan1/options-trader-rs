use anyhow::bail;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use std::fmt;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::mktdata::tt_api::FeedEvent;
use crate::mktdata::MktData;
use crate::positions::PriceEffect;
use crate::positions::StrategyType;
use crate::strategies::StrategyMeta;
use crate::web_client::WebClient;

use super::web_client::sessions::acc_api;

mod tt_api {
    use super::*;

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

    #[derive(Debug, Serialize, Deserialize)]
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

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Order {
        pub time_in_force: String,
        pub order_type: String,
        pub stop_trigger: Option<u32>,
        pub price: f64,
        pub price_effect: String,
        pub value: Option<u32>,
        pub value_effect: Option<String>,
        pub gtc_date: Option<String>,
        pub source: Option<String>,
        pub partition_key: Option<String>,
        pub preflight_id: Option<String>,
        pub automated_source: Option<bool>,
        pub legs: Vec<Leg>,
        pub rules: Option<Rules>,
        pub advanced_instructions: Option<AdvancedInstructions>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Leg {
        pub instrument_type: String,
        pub symbol: String,
        pub quantity: i32,
        pub action: String,
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Rules {
        pub route_after: String,
        pub cancel_at: String,
        pub conditions: Vec<Condition>,
    }

    #[derive(Debug, Serialize, Deserialize)]
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

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct PriceComponent {
        pub symbol: String,
        pub instrument_type: String,
        pub quantity: u32,
        pub quantity_direction: String,
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct AdvancedInstructions {
        pub strict_position_effect_validation: bool,
    }
}

#[derive(Debug)]
enum OrderType {
    Market,
    Limit,
    Stop,
    StopLimit,
}

impl fmt::Display for OrderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let order_type = match self {
            OrderType::Market => String::from("Market"),
            OrderType::Limit => String::from("Limit"),
            OrderType::Stop => String::from("Stop"),
            OrderType::StopLimit => String::from("StopLimit"),
        };
        write!(f, "{}", order_type)
    }
}

pub struct Orders {
    web_client: Arc<WebClient>,
    mkt_data: Arc<RwLock<MktData>>,
    orders: Vec<tt_api::Order>,
}

impl Orders {
    pub fn new(
        web_client: Arc<WebClient>,
        mkt_data: Arc<RwLock<MktData>>,
        cancel_token: CancellationToken,
    ) -> Self {
        let mut receiver = web_client.subscribe_to_events();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    msg = receiver.recv() => {
                        match msg {
                            Err(RecvError::Lagged(err)) => warn!("Publisher channel skipping a number of messages: {}", err),
                            Err(RecvError::Closed) => {
                                error!("Publisher channel closed");
                                cancel_token.cancel();
                            }
                            std::result::Result::Ok(val) => {
                                Self::handle_msg(val, &cancel_token);
                            }
                        }
                    }
                    _ = cancel_token.cancelled() => {
                        break
                    }
                }
            }
        });
        Self {
            web_client,
            mkt_data,
            orders: Vec::new(),
        }
    }

    pub async fn liquidate_position<Meta>(
        &self,
        meta_data: &Meta,
        price_effect: PriceEffect,
    ) -> Result<()>
    where
        Meta: StrategyMeta,
    {
        // check to see if order in flight
        if self.orders.iter().any(|order| {
            order.legs.iter().any(|leg| {
                meta_data
                    .get_symbols()
                    .iter()
                    .any(|symbol| *symbol == leg.symbol)
            })
        }) {
            info!("Order {} already in flight", meta_data.get_underlying());
            return anyhow::Result::Ok(());
        }
        // then build the order
        let mut order = Self::build_order_from_meta(meta_data, price_effect)?;

        // if not in flight find the midprice of strategy
        let midprice = Self::get_midprice(
            meta_data.get_position().strategy_type,
            &self.mkt_data,
            &order,
        )
        .await?;
        order.price = midprice;
        Self::place_order(self.web_client.get_account(), order, &self.web_client).await?;
        // then place the order
        anyhow::Result::Ok(())
    }

    fn build_order_from_meta<Meta>(
        meta_data: &Meta,
        price_effect: PriceEffect,
    ) -> Result<tt_api::Order, anyhow::Error>
    where
        Meta: StrategyMeta,
    {
        let order = tt_api::Order {
            time_in_force: String::from("DAY"),
            order_type: OrderType::Limit.to_string(),
            stop_trigger: None,
            price: 0.0,
            price_effect: PriceEffect::Debit.to_string(),
            value: None,
            value_effect: None,
            gtc_date: None,
            source: None,
            partition_key: None,
            preflight_id: None,
            automated_source: None,
            legs: meta_data
                .get_position()
                .legs
                .iter()
                .map(|leg| tt_api::Leg {
                    instrument_type: leg.instrument_type().to_string(),
                    symbol: leg.symbol().to_string(),
                    quantity: leg.quantity(),
                    action: String::from("Buy to Close"),
                })
                .collect(),
            rules: None,
            advanced_instructions: None,
        };
        Ok(order)
    }

    async fn get_midprice(
        strategy_type: StrategyType,
        mktdata: &Arc<RwLock<MktData>>,
        order: &tt_api::Order,
    ) -> Result<f64> {
        let reader = mktdata.read().await;
        let mut calculated_midprice = 0.0;
        match strategy_type {
            StrategyType::CreditSpread => {
                for leg in &order.legs {
                    let events = reader.get_snapshot_events(leg.symbol.as_str()).await;
                    let midprice = events
                        .iter()
                        .find(|event| match event {
                            FeedEvent::QuoteEvent(_) => true,
                            _ => false,
                        })
                        .and_then(|event| match event {
                            FeedEvent::QuoteEvent(quote) => {
                                Some((quote.bid_price + quote.ask_price) / 2.0)
                            }
                            _ => None,
                        })
                        .unwrap();
                    calculated_midprice += midprice;
                }
                return Ok(calculated_midprice);
            }
            _ => bail!("Invalid strategy type"),
        }
    }

    async fn place_order(
        account_number: &str,
        order: tt_api::Order,
        web_client: &Arc<WebClient>,
    ) -> Result<tt_api::OrderData> {
        web_client
            .post::<tt_api::Order, tt_api::OrderData>("orders", order)
            .await
    }

    async fn replace_order(
        account_number: &str,
        order: tt_api::Order,
        web_client: &Arc<WebClient>,
    ) -> Result<tt_api::OrderData> {
        web_client
            .put::<tt_api::Order, tt_api::OrderData>("orders", order)
            .await
    }

    fn handle_msg(msg: String, _cancel_token: &CancellationToken) {
        if let Ok(payload) = serde_json::from_str::<acc_api::Payload>(&msg) {
            if payload.msg_type.ne("Order") {
                return;
            }
            info!("msg received: {}", msg);
            if let Ok(msg) = serde_json::from_str::<acc_api::Payload>(&msg) {
                info!("Last mktdata message received, msg: {:?}", msg);
            }
        }
    }
}
