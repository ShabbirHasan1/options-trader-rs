use anyhow::Ok;
use anyhow::Result;
use percent_encoding::utf8_percent_encode;
use percent_encoding::AsciiSet;
use percent_encoding::CONTROLS;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::positions::InstrumentType;

use super::web_client::WebClient;

pub(crate) mod tt_api {
    use super::*;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Message {
        pub uid: String,
        #[serde(rename = "type")]
        pub message_type: String,
        pub channel: i32,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct FeedDataMessage {
        #[serde(flatten)]
        pub message: Message,
        pub data: Vec<FeedEvent>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub enum FeedEvent {
        QuoteEvent(QuoteEvent),
        GreeksEvent(GreeksEvent),
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct QuoteEvent {
        pub event_type: String,
        pub sequence: f64,
        pub time_nano_part: f64,
        pub bid_time: f64,
        pub bid_exchange_code: String,
        pub bid_price: f64,
        pub bid_size: f64,
        pub ask_time: f64,
        pub ask_exchange_code: String,
        pub ask_price: f64,
        pub ask_size: f64,
        pub event_symbol: String,
        pub event_time: f64,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct GreeksEvent {
        pub uid: String,
        pub event_type: String,
        pub event_flags: f64,
        pub index: f64,
        pub time: f64,
        pub sequence: f64,
        pub price: f64,
        pub volatility: f64,
        pub delta: f64,
        pub gamma: f64,
        pub theta: f64,
        pub rho: f64,
        pub vega: f64,
        pub event_symbol: String,
        pub event_time: f64,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct FutureOptionProduct {
        pub root_symbol: Option<String>,
        pub exchange: Option<String>,
        pub settlement_delay_days: Option<i32>,
        pub code: Option<String>,
        pub supported: Option<bool>,
        pub market_sector: Option<String>,
        pub product_type: Option<String>,
        pub expiration_type: Option<String>,
        pub display_factor: Option<String>,
        pub product_subtype: Option<String>,
        pub cash_settled: Option<bool>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct FutureOption {
        pub future_option_product: Option<FutureOptionProduct>,
        pub multiplier: Option<String>,
        pub root_symbol: String,
        pub exchange: Option<String>,
        pub notional_value: Option<String>,
        pub active: Option<bool>,
        pub is_closing_only: Option<bool>,
        pub underlying_symbol: String,
        pub maturity_date: Option<String>,
        pub is_exercisable_weekly: Option<bool>,
        pub strike_factor: Option<String>,
        pub product_code: Option<String>,
        pub days_to_expiration: Option<i32>,
        pub option_root_symbol: Option<String>,
        pub expiration_date: Option<String>,
        pub expires_at: Option<String>,
        pub last_trade_time: Option<String>,
        pub strike_price: Option<String>,
        pub is_primary_deliverable: Option<bool>,
        pub option_type: Option<String>,
        pub symbol: String,
        pub is_vanilla: Option<bool>,
        pub streamer_symbol: String,
        pub display_factor: Option<String>,
        pub stops_trading_at: Option<String>,
        pub exercise_style: Option<String>,
        pub is_confirmed: Option<bool>,
        pub future_price_ratio: Option<String>,
        pub settlement_type: Option<String>,
        pub underlying_count: Option<String>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct EquityOption {
        pub halted_at: Option<String>,
        pub instrument_type: String,
        pub root_symbol: String,
        pub active: Option<bool>,
        pub is_closing_only: Option<bool>,
        pub underlying_symbol: String,
        pub days_to_expiration: Option<i32>,
        pub expiration_date: Option<String>,
        pub expires_at: Option<String>,
        pub listed_market: Option<String>,
        pub strike_price: Option<String>,
        pub old_security_number: Option<String>,
        pub option_type: Option<String>,
        pub market_time_instrument_collection: Option<String>,
        pub symbol: Option<String>,
        pub streamer_symbol: String,
        pub expiration_type: Option<String>,
        pub shares_per_contract: Option<i32>,
        pub stops_trading_at: Option<String>,
        pub exercise_style: Option<String>,
        pub settlement_type: Option<String>,
        pub option_chain_type: Option<String>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct ResponseFutureOption {
        // pub message: Message,
        pub data: FutureOption,
    }
    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct ResponseEquityOption {
        // pub message: Message,
        pub data: EquityOption,
    }
}

const UTF8_ECODING: &AsciiSet = &CONTROLS.add(b' ').add(b'/');

struct Snapshot {
    symbol: String,
    streamer_symbol: String,
    mktdata: Option<tt_api::FeedDataMessage>,
}

pub(crate) struct MktData {
    web_client: Arc<WebClient>,
    events: Arc<Mutex<Vec<Snapshot>>>,
}

impl MktData {
    pub fn new(client: Arc<WebClient>, cancel_token: CancellationToken) -> Self {
        let mut receiver = client.subscribe_to_events();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut event_writer = Arc::clone(&events);
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
                                Self::handle_msg(&event_writer, val, &cancel_token).await;
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
            web_client: client,
            events,
        }
    }

    pub async fn subscribe_to_equity_mktdata(&mut self, symbol: &str) -> anyhow::Result<()> {
        self.web_client
            .subscribe_to_symbol(&symbol, "Quotes")
            .await?;
        Self::stash_subscription(&mut self.events, symbol, symbol).await;
        Ok(())
    }

    pub async fn subscribe_to_options_mktdata(
        &mut self,
        symbols: Vec<&str>,
        instrument_type: InstrumentType,
    ) -> anyhow::Result<()> {
        for symbol in symbols {
            let streamer_symbol = self.get_streamer_symbol(symbol, instrument_type).await?;
            info!(
                "Subscribing to mktdata events for symbol: {}",
                streamer_symbol
            );
            self.web_client
                .subscribe_to_symbol(&streamer_symbol, "Quotes")
                .await?;
            Self::stash_subscription(&mut self.events, symbol, &streamer_symbol).await;
        }
        Ok(())
    }

    pub async fn get_snapshot_data(&self, symbol: &str) -> Option<tt_api::FeedDataMessage> {
        self.events
            .lock()
            .await
            .iter()
            .find(|snapshot| snapshot.symbol.eq(symbol))
            .and_then(|snapshot| snapshot.mktdata.clone())
    }

    async fn get_streamer_symbol(
        &self,
        symbol: &str,
        instrument_type: InstrumentType,
    ) -> Result<String> {
        let symbol = utf8_percent_encode(symbol, UTF8_ECODING).to_string();
        let streamer_symbol = match instrument_type {
            InstrumentType::Equity => {
                self.web_client
                    .get::<tt_api::ResponseEquityOption>(&format!(
                        "instruments/equity-options/{}",
                        symbol
                    ))
                    .await?
                    .data
                    .streamer_symbol
            }
            InstrumentType::Future => {
                self.web_client
                    .get::<tt_api::ResponseFutureOption>(&format!(
                        "instruments/future-options/{}",
                        symbol
                    ))
                    .await?
                    .data
                    .streamer_symbol
            }
        };
        Ok(streamer_symbol)
    }

    async fn stash_subscription(
        events: &mut Arc<Mutex<Vec<Snapshot>>>,
        symbol: &str,
        streamer_symbol: &str,
    ) {
        let snapshot = Snapshot {
            symbol: symbol.to_string(),
            streamer_symbol: streamer_symbol.to_string(),
            mktdata: None,
        };
        events.lock().await.push(snapshot);
    }

    async fn handle_msg(
        events: &Arc<Mutex<Vec<Snapshot>>>,
        msg: String,
        _cancel_token: &CancellationToken,
    ) {
        if let serde_json::Result::Ok(msg) = serde_json::from_str::<tt_api::FeedDataMessage>(&msg) {
            info!("Last mktdata message received, msg: {:?}", msg);
            events.lock().await.iter_mut().for_each(|snapshot| {
                for event in &msg.data {
                    if let tt_api::FeedEvent::QuoteEvent(quote) = &event {
                        if snapshot.symbol.eq(&quote.event_symbol) {
                            snapshot.mktdata = Some(msg.clone());
                        }
                    }
                }
            });
        } else {
            info!("No Last mktdata message received, msg: {:?}", msg);
        }
    }
}
