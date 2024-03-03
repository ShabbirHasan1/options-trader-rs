use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::info;
use tracing::warn;

use super::web_client::WebClient;

mod tt_api {
    use super::*;

    #[derive(Debug, Serialize, Deserialize)]
    struct Message {
        uid: String,
        #[serde(rename = "type")]
        message_type: String,
        channel: i32,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub(crate) struct FeedDataMessage {
        uid: String,
        #[serde(rename = "type")]
        message_type: String,
        channel: i32,
        events: Vec<FeedEvent>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    enum FeedEvent {
        Quote(QuoteEvent),
        Profile(ProfileEvent),
        Trade(TradeEvent),
        TradeETH(TradeETHEvent),
        Candle(CandleEvent),
        Summary(SummaryEvent),
        TimeAndSale(TimeAndSaleEvent),
        Greeks(GreeksEvent),
        TheoPrice(TheoPriceEvent),
        Underlying(UnderlyingEvent),
        OptionSale(OptionSaleEvent),
        Series(SeriesEvent),
        Order(OrderEvent),
        SpreadOrder(SpreadOrderEvent),
        AnalyticOrder(AnalyticOrderEvent),
        Configuration(ConfigurationEvent),
        MessageEvent(MessageEvent),
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct QuoteEvent {}
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct ProfileEvent {}
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct TradeEvent {}
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct TradeETHEvent {}
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct CandleEvent {}
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct SummaryEvent {}
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct TimeAndSaleEvent {}
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct TheoPriceEvent {}
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct UnderlyingEvent {}
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct OptionSaleEvent {}
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct SeriesEvent {}
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct OrderEvent {}
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct SpreadOrderEvent {}
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct AnalyticOrderEvent {}
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct ConfigurationEvent {}
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct MessageEvent {}

    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct GreeksEvent {
        uid: String,
        #[serde(rename = "event-type")]
        event_type: String,
        #[serde(rename = "event-flags")]
        event_flags: f64,
        #[serde(rename = "index")]
        index: f64,
        #[serde(rename = "time")]
        time: f64,
        #[serde(rename = "sequence")]
        sequence: f64,
        #[serde(rename = "price")]
        price: f64,
        #[serde(rename = "volatility")]
        volatility: f64,
        #[serde(rename = "delta")]
        delta: f64,
        #[serde(rename = "gamma")]
        gamma: f64,
        #[serde(rename = "theta")]
        theta: f64,
        #[serde(rename = "rho")]
        rho: f64,
        #[serde(rename = "vega")]
        vega: f64,
        #[serde(rename = "event-symbol")]
        event_symbol: String,
        #[serde(rename = "event-time")]
        event_time: f64,
    }
}

pub(crate) struct MktData {
    web_client: Arc<WebClient>,
    event: HashMap<String, tt_api::FeedDataMessage>,
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
            event: HashMap::default(),
        }
    }

    pub async fn subscribe_to_mktdata(&self, symbol: &str) -> anyhow::Result<()> {
        info!("Subscribing to mktdata events for symbol: {}", symbol);
        self.web_client.subscribe_to_symbol(symbol).await
    }

    pub fn get_snapshot(&self, symbol: &str) -> tt_api::FeedDataMessage {
        self.event[symbol].clone()
    }

    fn handle_msg(msg: String, _cancel_token: &CancellationToken) {
        if let Ok(msg) = serde_json::from_str::<tt_api::FeedDataMessage>(&msg) {
            info!("Last mktdata message received, msg: {:?}", msg);
        }
    }
}
