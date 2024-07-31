use anyhow::Ok;
use anyhow::Result;
use rust_decimal::Decimal;
use std::fmt;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::mktdata::MktData;
use crate::mktdata::Snapshot;
use crate::positions::Direction;
use crate::positions::OptionType;
use crate::positions::PriceEffect;
use crate::positions::StrategyType;
use crate::strategies::StrategyMeta;
use crate::tt_api::mktdata::Quote;
use crate::tt_api::orders::*;
use crate::web_client::WebClient;

use super::web_client::sessions::acc_api;

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

    fn build_order_from_meta<Meta>(meta_data: &Meta, price_effect: PriceEffect) -> Result<Order>
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
            price_effect: PriceEffect::Debit.to_string(),
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

    async fn get_midprice(
        strategy_type: StrategyType,
        symbol: &str,
        mktdata: &Arc<RwLock<MktData>>,
        order: &Order,
    ) -> Result<Decimal> {
        fn get_mid_price(event: Option<Snapshot>) -> Decimal {
            if let Some(quote) = &event.as_ref().unwrap().quote {
                return quote.midprice();
            }
            Decimal::default()
        }

        let reader = mktdata.read().await;
        let calculated_midprice = match strategy_type {
            StrategyType::CreditSpread | StrategyType::CalendarSpread => {
                let sell_mid = get_mid_price(
                    reader
                        .get_snapshot_by_symbol::<Quote>(&order.legs[0].symbol)
                        .await,
                );
                let buy_mid = get_mid_price(
                    reader
                        .get_snapshot_by_symbol::<Quote>(&order.legs[1].symbol)
                        .await,
                );
                let mid = sell_mid - buy_mid;
                info!(
                    "New calc symbol:{} mid: {} sell mid: {} buy mid: {}",
                    symbol, mid, sell_mid, buy_mid
                );
                mid
            }
            StrategyType::IronCondor => {
                let call_sell_mid = get_mid_price(
                    reader
                        .get_snapshot_by_symbol::<Quote>(&order.legs[1].symbol)
                        .await,
                );
                let call_buy_mid = get_mid_price(
                    reader
                        .get_snapshot_by_symbol::<Quote>(&order.legs[0].symbol)
                        .await,
                );
                let put_sell_mid = get_mid_price(
                    reader
                        .get_snapshot_by_symbol::<Quote>(&order.legs[2].symbol)
                        .await,
                );
                let put_buy_mid = get_mid_price(
                    reader
                        .get_snapshot_by_symbol::<Quote>(&order.legs[3].symbol)
                        .await,
                );
                let mid = (call_sell_mid - call_buy_mid) + (put_sell_mid - put_buy_mid);
                info!(
                    "New calc mid: {} call spread: {} put spread: {}",
                    mid,
                    call_sell_mid - call_buy_mid,
                    put_sell_mid - put_buy_mid
                );
                mid
            }
            _ => Decimal::default(),
        };
        debug!(
            "For leg symbol: {}, calculated midprice: {}",
            order.legs[0].symbol, calculated_midprice,
        );

        Ok(calculated_midprice)
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
