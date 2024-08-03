use anyhow::Ok;
use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::fmt;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

use super::mktdata::*;
use super::positions::*;
use crate::connectivity::web_client::WebClient;
use crate::strategy::monitor::StrategyMeta;
use crate::tt_api::{mktdata::Quote, orders::*, sessions::Payload};

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

async fn calculate_midprice(
    strategy_type: StrategyType,
    symbol: &str,
    mktdata: &Arc<RwLock<MktData>>,
    order: &Order,
) -> Result<Decimal> {
    fn get_mid_price(snapshot: &Snapshot) -> Decimal {
        if let Some(quote) = &snapshot.quote {
            return quote.midprice();
        }
        Decimal::default()
    }

    let reader = mktdata.read().await;
    let calculated_midprice = match strategy_type {
        StrategyType::CreditSpread | StrategyType::CalendarSpread => {
            let snapshots = reader.group_snapshots_by_underlying::<Quote>(symbol).await;
            let mid_price = get_mid_price(&snapshots[0]) + get_mid_price(&snapshots[1]) / dec!(2);

            info!("New calc symbol:{} mid: {}", symbol, mid_price);
            mid_price
        }
        StrategyType::IronCondor => {
            let snapshots = reader.group_snapshots_by_underlying::<Quote>(symbol).await;
            let call_mid_price =
                get_mid_price(&snapshots[0]) + get_mid_price(&snapshots[1]) / dec!(2);

            let put_mid_price =
                get_mid_price(&snapshots[2]) + get_mid_price(&snapshots[3]) / dec!(2);

            let mid_price = call_mid_price + put_mid_price / dec!(2);
            info!(
                "New calc mid: {} call spread: {} put spread: {}",
                mid_price, call_mid_price, put_mid_price,
            );
            mid_price
        }
        _ => Decimal::default(),
    };
    debug!(
        "For leg symbol: {}, calculated midprice: {}",
        order.legs[0].symbol, calculated_midprice,
    );

    Ok(calculated_midprice)
}

fn build_order<Meta>(meta_data: &Meta, price_effect: PriceEffect) -> Result<Order>
where
    Meta: StrategyMeta,
{
    fn get_action(direction: Direction) -> String {
        match direction {
            Direction::Long => String::from("Sell to Close"),
            Direction::Short => String::from("Buy to Close"),
        }
    }

    fn get_symbol(symbol: &str, instrument_type: OptionType) -> String {
        match instrument_type {
            OptionType::FutureOption => symbol.to_string(),
            _ => symbol.to_string(),
        }
    }

    let order = Order {
        time_in_force: String::from("DAY"),
        order_type: OrderType::Limit.to_string(),
        price_effect: price_effect.to_string(),
        legs: meta_data
            .get_position()
            .legs
            .iter()
            .map(|leg| Leg {
                instrument_type: leg.option_type.to_string(),
                symbol: get_symbol(&leg.symbol, leg.option_type),
                quantity: leg.quantity,
                action: get_action(leg.direction),
            })
            .collect(),
        ..Default::default()
    };
    info!("Order: {:?}", order);
    Ok(order)
}

pub struct Orders {
    web_client: Arc<WebClient>,
    mkt_data: Arc<RwLock<MktData>>,
    orders: Vec<Order>,
}

impl Orders {
    pub fn new(
        web_client: Arc<WebClient>,
        mkt_data: Arc<RwLock<MktData>>,
        cancel_token: CancellationToken,
    ) -> Self {
        let mut receiver = web_client.subscribe_acc_events();
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

    fn has_order_in_flight<Meta>(&self, meta_data: &Meta) -> bool
    where
        Meta: StrategyMeta,
    {
        if self.orders.iter().any(|order| {
            order.legs.iter().any(|leg| {
                meta_data
                    .get_symbols()
                    .iter()
                    .any(|symbol| *symbol == leg.symbol)
            })
        }) {
            debug!("Order {} already in flight", meta_data.get_underlying());
            return true;
        }
        false
    }

    pub async fn open_position<Meta>(
        &mut self,
        meta_data: &Meta,
        price_effect: PriceEffect,
    ) -> Result<()>
    where
        Meta: StrategyMeta,
    {
        // check to see if order in flight
        if self.has_order_in_flight(meta_data) {
            return Ok(());
        }

        let mut order = build_order(meta_data, price_effect)?;

        // if not in flight find the midprice of strategy
        let midprice = calculate_midprice(
            meta_data.get_position().strategy_type,
            meta_data.get_underlying(),
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
            warn!("Failed to calculate midprice");
            return Ok(());
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

    async fn place_order(
        account_number: &str,
        order: &Order,
        web_client: &Arc<WebClient>,
    ) -> Result<OrderData> {
        info!("Placing order: {:?}", order);
        web_client
            .post::<Order, OrderData>(
                &format!("accounts/{}/orders/dry-run", account_number),
                order.clone(),
            )
            .await
    }

    async fn replace_order(
        account_number: &str,
        order: Order,
        web_client: &Arc<WebClient>,
    ) -> Result<OrderData> {
        // web_client
        //     .put::<Order, OrderData>(
        //         &format!(
        //             "accounts/{}/orders/{}/dry-run",
        //             account_number,
        //             order.preflight_id.clone().unwrap()
        //         ),
        //         order,
        //     )
        //     .await
        Ok(OrderData::default())
    }

    fn handle_msg(msg: String, _cancel_token: &CancellationToken) {
        if let serde_json::Result::Ok(payload) = serde_json::from_str::<Payload>(&msg) {
            if payload.msg_type.ne("Order") {
                return;
            }
            info!("msg received: {}", msg);
            if let serde_json::Result::Ok(msg) = serde_json::from_str::<Payload>(&msg) {
                info!("Last mktdata message received, msg: {:?}", msg);
            }
        }
    }
}
