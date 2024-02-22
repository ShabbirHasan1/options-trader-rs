use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::web_client::WebClient;

use self::tt_api::OrderData;

mod tt_api {
    use super::*;

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Leg {
        #[serde(rename = "instrument-type")]
        pub instrument_type: String,
        pub symbol: String,
        pub quantity: i32,
        #[serde(rename = "remaining-quantity")]
        pub remaining_quantity: i32,
        pub action: String,
        pub fills: Vec<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct OrderData {
        pub id: i32,
        #[serde(rename = "account-number")]
        pub account_number: String,
        #[serde(rename = "time-in-force")]
        pub time_in_force: String,
        #[serde(rename = "order-type")]
        pub order_type: String,
        pub size: i32,
        #[serde(rename = "underlying-symbol")]
        pub underlying_symbol: String,
        #[serde(rename = "underlying-instrument-type")]
        pub underlying_instrument_type: String,
        pub status: String,
        pub cancellable: bool,
        pub editable: bool,
        pub edited: bool,
        pub legs: Option<Vec<Leg>>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Payload {
        #[serde(rename = "type")]
        pub msg_type: String,
        pub data: OrderData,
    }
}

pub struct Orders {
    web_client: Arc<WebClient>,
    orders: Vec<OrderData>,
}

impl Orders {
    pub fn new(web_client: Arc<WebClient>, cancel_token: CancellationToken) -> Self {
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
            orders: Vec::new(),
        }
    }

    pub async fn liquidate_position(&self) -> Result<OrderData> {
        self.web_client.get::<OrderData>("orders").await
    }

    fn handle_msg(msg: String, _cancel_token: &CancellationToken) {
        info!("msg received: {}", msg);
        if let Ok(msg) = serde_json::from_str::<tt_api::Payload>(&msg) {
            info!("Last mktdata message received, msg: {:?}", msg);
        }
    }
}
