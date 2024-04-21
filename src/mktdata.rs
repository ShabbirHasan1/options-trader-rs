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
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::mktdata::tt_api::FeedEvent;
use crate::positions::InstrumentType;

use super::web_client::WebClient;

pub(crate) mod tt_api {
    use rust_decimal::Decimal;

    use super::*;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Message {
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
    #[serde(tag = "eventType")]
    pub enum FeedEvent {
        #[serde(rename = "Quote")]
        QuoteEvent(Quote),
        #[serde(rename = "Greeks")]
        GreeksEvent(Greeks),
    }

    impl PartialEq for FeedEvent {
        fn eq(&self, other: &Self) -> bool {
            matches!(
                (self, other),
                (FeedEvent::QuoteEvent(_), FeedEvent::QuoteEvent(_))
                    | (FeedEvent::GreeksEvent(_), FeedEvent::GreeksEvent(_))
            )
        }
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Quote {
        pub event_symbol: String,
        pub event_time: f64,
        pub sequence: f64,
        pub time_nano_part: f64,
        pub bid_time: f64,
        pub bid_exchange_code: String,
        #[serde(with = "rust_decimal::serde::float")]
        pub bid_price: Decimal,
        pub bid_size: f64,
        pub ask_time: f64,
        pub ask_exchange_code: String,
        #[serde(with = "rust_decimal::serde::float")]
        pub ask_price: Decimal,
        pub ask_size: f64,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Greeks {
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

#[derive(Clone, Debug)]
struct Snapshot {
    symbol: String,
    underlying: String,
    streamer_symbol: String,
    mktdata: Vec<tt_api::FeedEvent>,
}

pub(crate) struct MktData {
    web_client: Arc<WebClient>,
    events: Arc<Mutex<Vec<Snapshot>>>,
}

impl MktData {
    pub fn new(client: Arc<WebClient>, cancel_token: CancellationToken) -> Self {
        let mut receiver = client.subscribe_to_events();
        let events = Arc::new(Mutex::new(Vec::new()));
        let event_writer = Arc::clone(&events);
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

    pub async fn subscribe_to_equity_mktdata(
        &mut self,
        symbol: &str,
        instrument_type: InstrumentType,
    ) -> anyhow::Result<()> {
        if let InstrumentType::Future = instrument_type {
            return Ok(());
        }
        self.web_client
            .subscribe_to_symbol(&symbol, vec!["Quote"])
            .await?;
        Self::stash_subscription(&mut self.events, symbol, symbol, symbol).await;
        Ok(())
    }

    pub async fn subscribe_to_options_mktdata(
        &mut self,
        symbol: &str,
        underlying: &str,
        instrument_type: InstrumentType,
    ) -> anyhow::Result<()> {
        if let InstrumentType::Future = instrument_type {
            return Ok(());
        }
        let streamer_symbol = self.get_streamer_symbol(symbol, instrument_type).await?;
        info!(
            "Subscribing to mktdata events for symbol: {}",
            streamer_symbol
        );
        self.web_client
            .subscribe_to_symbol(&streamer_symbol, vec!["Quote", "Greeks"])
            .await?;
        Self::stash_subscription(&mut self.events, symbol, underlying, &streamer_symbol).await;
        Ok(())
    }

    pub async fn get_snapshot_events(&self, symbol: &str) -> Vec<FeedEvent> {
        let reader = self.events.lock().await;
        let result = reader
            .iter()
            .find(|snapshot| snapshot.symbol.eq(symbol))
            .map(|snapshot| snapshot.mktdata.clone())
            .unwrap_or_default();

        debug!("Result {:?}", result);
        result
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
        underlying: &str,
        streamer_symbol: &str,
    ) {
        let snapshot = Snapshot {
            symbol: symbol.to_string(),
            underlying: underlying.to_string(),
            streamer_symbol: streamer_symbol.to_string(),
            mktdata: Vec::new(),
        };
        events.lock().await.push(snapshot);
    }

    async fn handle_msg(
        events: &Arc<Mutex<Vec<Snapshot>>>,
        msg: String,
        _cancel_token: &CancellationToken,
    ) {
        fn get_symbol(data: &FeedEvent) -> String {
            match data {
                FeedEvent::QuoteEvent(event) => event.event_symbol.clone(),
                FeedEvent::GreeksEvent(event) => event.event_symbol.clone(),
            }
        }

        match serde_json::from_str::<tt_api::FeedDataMessage>(&msg) {
            serde_json::Result::Ok(mut msg) => {
                debug!("Last mktdata message received, msg: {:?}", msg);

                let mut writer = events.lock().await;
                writer.iter_mut().for_each(|snapshot| {
                    msg.data.iter_mut().for_each(|event| {
                        if get_symbol(event).ne(&snapshot.streamer_symbol) {
                            return;
                        }
                        if snapshot.mktdata.is_empty() {
                            snapshot.mktdata.push(event.clone());
                        } else {
                            snapshot.mktdata.iter_mut().for_each(|data| {
                                match (&event, &data) {
                                    (FeedEvent::QuoteEvent(_), FeedEvent::QuoteEvent(_)) => {
                                        *data = event.clone()
                                    }
                                    (FeedEvent::GreeksEvent(_), FeedEvent::GreeksEvent(_)) => {
                                        *data = event.clone()
                                    }
                                    _ => (),
                                };
                            })
                        }
                    })
                });
            }
            serde_json::Result::Err(err) => {
                info!("No Last mktdata message received, error: {:?}", err);
            }
        };
        debug!("Writer updated {}", events.lock().await.len());
    }
}
