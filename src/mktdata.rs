use anyhow::bail;
use anyhow::Ok;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
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
        #[serde(rename = "type")]
        pub message: Message,
        pub data: Vec<CandleData>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct CandleData {
        #[serde(rename = "eventType")]
        pub event_type: Option<String>,
        #[serde(rename = "eventSymbol")]
        pub event_symbol: Option<String>,
        #[serde(rename = "eventTime")]
        pub event_time: Option<i64>,
        #[serde(rename = "eventFlags")]
        pub event_flags: Option<i64>,
        pub index: Option<i64>,
        pub time: Option<i64>,
        pub sequence: Option<i64>,
        pub count: Option<i64>,
        pub open: Option<f64>,
        pub high: Option<f64>,
        pub low: Option<f64>,
        pub close: Option<f64>,
        pub volume: Option<f64>,
        pub vwap: Option<String>,
        #[serde(rename = "bidVolume")]
        pub bid_volume: Option<String>,
        #[serde(rename = "askVolume")]
        pub ask_volume: Option<String>,
        #[serde(rename = "impVolatility")]
        pub imp_volatility: Option<String>,
        #[serde(rename = "openInterest")]
        pub open_interest: Option<String>,
    }

    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub enum FeedEvent {
    //     Quote(QuoteEvent),
    //     Profile(ProfileEvent),
    //     Trade(TradeEvent),
    //     TradeETH(TradeETHEvent),
    //     Candle(CandleEvent),
    //     Summary(SummaryEvent),
    //     TimeAndSale(TimeAndSaleEvent),
    //     Greeks(GreeksEvent),
    //     TheoPrice(TheoPriceEvent),
    //     Underlying(UnderlyingEvent),
    //     OptionSale(OptionSaleEvent),
    //     Series(SeriesEvent),
    //     Order(OrderEvent),
    //     SpreadOrder(SpreadOrderEvent),
    //     AnalyticOrder(AnalyticOrderEvent),
    //     Configuration(ConfigurationEvent),
    //     MessageEvent(MessageEvent),
    // }

    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct QuoteEvent {}
    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct ProfileEvent {}
    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct TradeEvent {}
    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct TradeETHEvent {}
    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct CandleEvent {}
    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct SummaryEvent {}
    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct TimeAndSaleEvent {}
    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct TheoPriceEvent {}
    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct UnderlyingEvent {}
    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct OptionSaleEvent {}
    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct SeriesEvent {}
    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct OrderEvent {}
    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct SpreadOrderEvent {}
    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct AnalyticOrderEvent {}
    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct ConfigurationEvent {}
    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct MessageEvent {}

    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct GreeksEvent {
    //     pub uid: String,
    //     #[serde(rename = "eventType")]
    //     pub event_type: String,
    //     #[serde(rename = "event-flags")]
    //     pub event_flags: f64,
    //     #[serde(rename = "index")]
    //     pub index: f64,
    //     #[serde(rename = "time")]
    //     pub time: f64,
    //     #[serde(rename = "sequence")]
    //     pub sequence: f64,
    //     #[serde(rename = "price")]
    //     pub price: f64,
    //     #[serde(rename = "volatility")]
    //     pub volatility: f64,
    //     #[serde(rename = "delta")]
    //     pub delta: f64,
    //     #[serde(rename = "gamma")]
    //     pub gamma: f64,
    //     #[serde(rename = "theta")]
    //     pub theta: f64,
    //     #[serde(rename = "rho")]
    //     pub rho: f64,
    //     #[serde(rename = "vega")]
    //     pub vega: f64,
    //     #[serde(rename = "eventSymbol")]
    //     pub event_symbol: String,
    //     #[serde(rename = "event-time")]
    //     pub event_time: f64,
    // }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct FutureOptionProduct {
        #[serde(rename = "root-symbol")]
        pub root_symbol: Option<String>,
        pub exchange: Option<String>,
        #[serde(rename = "settlement-delay-days")]
        pub settlement_delay_days: Option<i32>,
        pub code: Option<String>,
        pub supported: Option<bool>,
        #[serde(rename = "market-sector")]
        pub market_sector: Option<String>,
        #[serde(rename = "product-type")]
        pub product_type: Option<String>,
        #[serde(rename = "expiration-type")]
        pub expiration_type: Option<String>,
        #[serde(rename = "display-factor")]
        pub display_factor: Option<i32>,
        #[serde(rename = "product-subtype")]
        pub product_subtype: Option<String>,
        #[serde(rename = "cash-settled")]
        pub cash_settled: Option<bool>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct FutureOption {
        #[serde(rename = "future-option-product")]
        pub future_option_product: Option<FutureOptionProduct>,
        pub multiplier: Option<i32>,
        #[serde(rename = "root-symbol")]
        pub root_symbol: String,
        pub exchange: Option<String>,
        #[serde(rename = "notional-value")]
        pub notional_value: Option<i32>,
        pub active: Option<bool>,
        #[serde(rename = "is-closing-only")]
        pub is_closing_only: Option<bool>,
        #[serde(rename = "underlying-symbol")]
        pub underlying_symbol: String,
        #[serde(rename = "maturity-date")]
        pub maturity_date: Option<String>,
        #[serde(rename = "is-exercisable-weekly")]
        pub is_exercisable_weekly: Option<bool>,
        #[serde(rename = "strike-factor")]
        pub strike_factor: Option<i32>,
        #[serde(rename = "product-code")]
        pub product_code: Option<String>,
        #[serde(rename = "days-to-expiration")]
        pub days_to_expiration: Option<i32>,
        #[serde(rename = "option-root-symbol")]
        pub option_root_symbol: Option<String>,
        #[serde(rename = "expiration-date")]
        pub expiration_date: Option<String>,
        #[serde(rename = "expires-at")]
        pub expires_at: Option<String>,
        #[serde(rename = "last-trade-time")]
        pub last_trade_time: Option<String>,
        #[serde(rename = "strike-price")]
        pub strike_price: Option<i32>,
        #[serde(rename = "is-primary-deliverable")]
        pub is_primary_deliverable: Option<bool>,
        #[serde(rename = "option-type")]
        pub option_type: Option<String>,
        pub symbol: String,
        #[serde(rename = "is-vanilla")]
        pub is_vanilla: Option<bool>,
        #[serde(rename = "streamer-symbol")]
        pub streamer_symbol: String,
        #[serde(rename = "display-factor")]
        pub display_factor: Option<i32>,
        #[serde(rename = "stops-trading-at")]
        pub stops_trading_at: Option<String>,
        #[serde(rename = "exercise-style")]
        pub exercise_style: Option<String>,
        #[serde(rename = "is-confirmed")]
        pub is_confirmed: Option<bool>,
        #[serde(rename = "future-price-ratio")]
        pub future_price_ratio: Option<i32>,
        #[serde(rename = "settlement-type")]
        pub settlement_type: Option<String>,
        #[serde(rename = "underlying-count")]
        pub underlying_count: Option<i32>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct EquityOption {
        #[serde(rename = "halted-at")]
        pub halted_at: Option<String>,
        #[serde(rename = "instrument-type")]
        pub instrument_type: String,
        #[serde(rename = "root-symbol")]
        pub root_symbol: String,
        pub active: Option<bool>,
        #[serde(rename = "is-closing-only")]
        pub is_closing_only: Option<bool>,
        #[serde(rename = "underlying-symbol")]
        pub underlying_symbol: String,
        #[serde(rename = "days-to-expiration")]
        pub days_to_expiration: Option<i32>,
        #[serde(rename = "expiration-date")]
        pub expiration_date: Option<String>,
        #[serde(rename = "expires-at")]
        pub expires_at: Option<String>,
        #[serde(rename = "listed-market")]
        pub listed_market: Option<String>,
        #[serde(rename = "strike-price")]
        pub strike_price: Option<String>,
        #[serde(rename = "old-security-number")]
        pub old_security_number: Option<String>,
        #[serde(rename = "option-type")]
        pub option_type: Option<String>,
        #[serde(rename = "market-time-instrument-collection")]
        pub market_time_instrument_collection: Option<String>,
        pub symbol: Option<String>,
        #[serde(rename = "streamer-symbol")]
        pub streamer_symbol: String,
        #[serde(rename = "expiration-type")]
        pub expiration_type: Option<String>,
        #[serde(rename = "shares-per-contract")]
        pub shares_per_contract: Option<i32>,
        #[serde(rename = "stops-trading-at")]
        pub stops_trading_at: Option<String>,
        #[serde(rename = "exercise-style")]
        pub exercise_style: Option<String>,
        #[serde(rename = "settlement-type")]
        pub settlement_type: Option<String>,
        #[serde(rename = "option-chain-type")]
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

struct Snapshot {
    symbol: String,
    streamer_symbol: String,
    mktdata: Option<tt_api::FeedDataMessage>,
}

pub(crate) struct MktData {
    web_client: Arc<WebClient>,
    events: HashMap<String, Vec<Snapshot>>,
}

impl MktData {
    pub fn new(client: Arc<WebClient>, cancel_token: CancellationToken) -> Self {
        let mut receiver = client.subscribe_to_events();
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
            web_client: client,
            events: HashMap::default(),
        }
    }

    async fn get_streamer_symbol(
        &self,
        symbol: &str,
        instrument_type: InstrumentType,
    ) -> Result<String> {
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

    fn stash_subscription(
        event_map: &mut HashMap<String, Vec<Snapshot>>,
        underlying_symbol: &str,
        symbol: &str,
        streamer_symbol: &str,
    ) {
        let snapshot = Snapshot {
            symbol: symbol.to_string(),
            streamer_symbol: streamer_symbol.to_string(),
            mktdata: None,
        };
        if event_map.contains_key(underlying_symbol) {
            event_map
                .get_mut(underlying_symbol)
                .expect(&format!(
                    "can't find entry for underlying {}",
                    underlying_symbol
                ))
                .push(snapshot);
        } else {
            event_map.insert(underlying_symbol.to_string(), vec![snapshot]);
        }
    }

    pub async fn subscribe_to_mktdata(
        &mut self,
        underlying_symbol: &str,
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
                .subscribe_to_symbol(&streamer_symbol)
                .await?;
            Self::stash_subscription(
                &mut self.events,
                underlying_symbol,
                symbol,
                &streamer_symbol,
            );
        }
        Ok(())
    }

    pub fn get_snapshot_data(
        &self,
        underlying_symbol: &str,
        symbol: &str,
    ) -> Option<tt_api::FeedDataMessage> {
        self.events[underlying_symbol]
            .iter()
            .find(|snapshot| snapshot.symbol.eq(symbol))
            .and_then(|snapshot| snapshot.mktdata.clone())
    }

    fn handle_msg(msg: String, _cancel_token: &CancellationToken) {
        if let serde_json::Result::Ok(msg) = serde_json::from_str::<tt_api::FeedDataMessage>(&msg) {
            info!("Last mktdata message received, msg: {:?}", msg);
        } else {
            info!("No Last mktdata message received, msg: {:?}", msg);
        }
    }
}
