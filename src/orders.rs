use anyhow::bail;
use anyhow::Ok;
use anyhow::Result;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde::Serialize;
use std::fmt;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::instrument;
use tracing::warn;

use crate::mktdata::tt_api::FeedEvent;
use crate::mktdata::tt_api::Future;
use crate::mktdata::MktData;
use crate::positions::Direction;
use crate::positions::InstrumentType;
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
        &mut self,
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
            debug!("Order {} already in flight", meta_data.get_underlying());
            return Ok(());
        }

        let mut order = Self::build_order_from_meta(meta_data, price_effect)?;

        // if not in flight find the midprice of strategy
        let midprice = Self::get_midprice(
            meta_data.get_position().strategy_type,
            &self.mkt_data,
            &order,
        )
        .await?;
        info!(
            "For symbol: {}, Calculated midprice: {}",
            meta_data.get_underlying(),
            midprice,
        );

        if midprice.eq(&Decimal::ZERO) {
            bail!("Failed to calculate midprice")
        }

        info!(
            "Calling liquidate position for {}",
            meta_data.get_underlying()
        );
        // then build the order
        order.price = midprice;
        if let Err(err) =
            Self::place_order(self.web_client.get_account(), &order, &self.web_client).await
        {
            error!("Failed to place order, error: {}", err);
            return Err(err);
        }
        self.orders.push(order);
        Ok(())
    }

    fn build_order_from_meta<Meta>(
        meta_data: &Meta,
        price_effect: PriceEffect,
    ) -> Result<tt_api::Order>
    where
        Meta: StrategyMeta,
    {
        fn get_action(direction: Direction) -> String {
            match direction {
                Direction::Long => String::from("Sell to Close"),
                Direction::Short => String::from("Buy to Close"),
            }
        }

        fn get_symbol(symbol: &str, instrument_type: InstrumentType) -> String {
            match instrument_type {
                InstrumentType::FutureOption => symbol.to_string(),
                _ => symbol.to_string(),
            }
        }

        let order = tt_api::Order {
            time_in_force: String::from("DAY"),
            order_type: OrderType::Limit.to_string(),
            price_effect: PriceEffect::Debit.to_string(),
            legs: meta_data
                .get_position()
                .legs
                .iter()
                .map(|leg| tt_api::Leg {
                    instrument_type: leg.instrument_type().to_string(),
                    symbol: get_symbol(leg.symbol(), leg.instrument_type()),
                    quantity: leg.quantity(),
                    action: get_action(leg.direction()),
                })
                .collect(),
            ..Default::default()
        };
        info!("Order: {:?}", order);
        Ok(order)
    }

    async fn get_midprice(
        strategy_type: StrategyType,
        mktdata: &Arc<RwLock<MktData>>,
        order: &tt_api::Order,
    ) -> Result<Decimal> {
        fn get_bid_ask(events: Vec<FeedEvent>) -> (Decimal, Decimal) {
            events
                .iter()
                .find(|event| matches!(event, FeedEvent::QuoteEvent(_)))
                .and_then(|event| match event {
                    FeedEvent::QuoteEvent(quote) => {
                        Some((quote.bid_price.abs(), quote.ask_price.abs()))
                    }
                    _ => None,
                })
                .unwrap_or((Decimal::default(), Decimal::default()))
        }

        let reader = mktdata.read().await;
        let calculated_midprice = match strategy_type {
            StrategyType::CreditSpread | StrategyType::CalendarSpread => {
                let (sell_bid, sell_ask) =
                    get_bid_ask(reader.get_snapshot_events(&order.legs[0].symbol).await);
                let (buy_bid, buy_ask) =
                    get_bid_ask(reader.get_snapshot_events(&order.legs[1].symbol).await);

                let option_ask = sell_ask - buy_bid;
                let option_bid = sell_bid - buy_ask;
                let mid = (option_ask + option_bid) / Decimal::new(2, 0);
                info!(
                    "New calc mid: {} option bid: {} option ask: {}",
                    mid, option_bid, option_ask
                );
                mid
            }
            StrategyType::IronCondor => {
                let (call_sell_bid, call_sell_ask) =
                    get_bid_ask(reader.get_snapshot_events(&order.legs[0].symbol).await);
                let (call_buy_bid, call_buy_ask) =
                    get_bid_ask(reader.get_snapshot_events(&order.legs[2].symbol).await);

                let (put_sell_bid, put_sell_ask) =
                    get_bid_ask(reader.get_snapshot_events(&order.legs[1].symbol).await);
                let (put_buy_bid, put_buy_ask) =
                    get_bid_ask(reader.get_snapshot_events(&order.legs[3].symbol).await);

                let option_ask = (call_sell_ask - call_buy_bid) + (put_sell_ask - put_buy_bid);
                let option_bid = (call_sell_bid - call_buy_ask) + (put_sell_bid - put_buy_ask);
                let mid = (option_ask + option_bid) / Decimal::new(2, 0);
                info!(
                    "New calc mid: {} option bid: {} option ask: {}",
                    mid, option_bid, option_ask
                );
                mid
            }
            _ => Decimal::default(),
        };
        info!(
            "For symbol: {}, calculated midprice: {}",
            order.legs[0].symbol, calculated_midprice,
        );
        if calculated_midprice.eq(&Decimal::ZERO) {
            bail!("Failed to calculate midprice")
        }

        Ok(calculated_midprice)
    }

    async fn place_order(
        account_number: &str,
        order: &tt_api::Order,
        web_client: &Arc<WebClient>,
    ) -> Result<tt_api::OrderData> {
        info!("Placing order: {:?}", order);
        web_client
            .post::<tt_api::Order, tt_api::OrderData>(
                &format!("accounts/{}/orders/dry-run", account_number),
                order.clone(),
            )
            .await
    }

    async fn replace_order(
        account_number: &str,
        order: tt_api::Order,
        web_client: &Arc<WebClient>,
    ) -> Result<tt_api::OrderData> {
        // web_client
        //     .put::<tt_api::Order, tt_api::OrderData>(
        //         &format!(
        //             "accounts/{}/orders/{}/dry-run",
        //             account_number,
        //             order.preflight_id.clone().unwrap()
        //         ),
        //         order,
        //     )
        //     .await
        Ok(tt_api::OrderData::default())
    }

    fn handle_msg(msg: String, _cancel_token: &CancellationToken) {
        if let serde_json::Result::Ok(payload) = serde_json::from_str::<acc_api::Payload>(&msg) {
            if payload.msg_type.ne("Order") {
                return;
            }
            info!("msg received: {}", msg);
            if let serde_json::Result::Ok(msg) = serde_json::from_str::<acc_api::Payload>(&msg) {
                info!("Last mktdata message received, msg: {:?}", msg);
            }
        }
    }
}
