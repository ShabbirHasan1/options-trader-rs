use anyhow::anyhow;
use anyhow::Ok;
use anyhow::Result;
use core::fmt;
use percent_encoding::utf8_percent_encode;
use percent_encoding::AsciiSet;
use percent_encoding::CONTROLS;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::positions::InstrumentType;
use crate::tt_api::mkt_data::*;

use super::web_client::WebClient;

const UTF8_ECODING: &AsciiSet = &CONTROLS.add(b' ').add(b'/');

pub(crate) trait FeedEventExt {
    type Event;
    fn extract_event(snapshot: &Snapshot) -> Option<Self::Event>;
}

impl FeedEventExt for Quote {
    type Event = Quote;

    fn extract_event(snapshot: &Snapshot) -> Option<Self::Event> {
        snapshot.quote.clone()
    }
}

impl FeedEventExt for Greeks {
    type Event = Greeks;

    fn extract_event(snapshot: &Snapshot) -> Option<Self::Event> {
        snapshot.greeks.clone()
    }
}

#[derive(Clone, Debug)]
struct Snapshot {
    symbol: String,
    underlying: String,
    streamer_symbol: String,
    last_update: Instant,
    strike_price: Option<Decimal>,
    quote: Option<Quote>,
    greeks: Option<Greeks>,
}

pub(crate) struct MktData {
    web_client: Arc<WebClient>,
    events: Arc<Mutex<Vec<Snapshot>>>,
}

impl MktData {
    pub fn new(client: Arc<WebClient>, cancel_token: CancellationToken) -> Self {
        let mut receiver = client.subscribe_md_events();
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
                                Self::handle_msg(&event_writer, val, &cancel_token).await
                            }
                        }
                    }
                    _ = sleep(Duration::from_secs(1)) => {
                        event_writer.lock().await.iter_mut().for_each(|snapshot| {
                            let instant = &mut snapshot.last_update;
                            if Instant::now().duration_since(*instant).gt(&Duration::from_secs(30)) {
                                warn!("Not received any mktdata for symbol: {} for 30 seconds", snapshot.streamer_symbol);
                                instant.clone_from(&Instant::now());
                            }
                        })
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

    async fn handle_msg(
        events: &Arc<Mutex<Vec<Snapshot>>>,
        msg: String,
        _cancel_token: &CancellationToken,
    ) {
        fn get_symbol(data: &FeedEvent) -> &str {
            match data {
                FeedEvent::QuoteEvent(event) => event.event_symbol.as_ref(),
                FeedEvent::GreeksEvent(event) => event.event_symbol.as_ref(),
            }
        }

        match serde_json::from_str::<FeedDataMessage>(&msg) {
            serde_json::Result::Ok(mut msg) => {
                debug!("Last mktdata message received, msg: {:?}", msg);

                let mut writer = events.lock().await;
                writer.iter_mut().for_each(|snapshot| {
                    msg.data.iter_mut().for_each(|event| {
                        let symbol = get_symbol(event);
                        if symbol.ne(&snapshot.streamer_symbol) {
                            return;
                        }
                        match &event {
                            FeedEvent::QuoteEvent(event) => {
                                snapshot.quote = Some(event.clone());
                            }
                            FeedEvent::GreeksEvent(event) => {
                                snapshot.greeks = Some(event.clone());
                            }
                            _ => (),
                        }
                        snapshot.last_update = Instant::now();
                    })
                });
            }
            serde_json::Result::Err(err) => {
                warn!(
                    "No Last mktdata message received: {:?}, error: {:?}",
                    msg, err
                );
            }
        };
        debug!("Writer updated {}", events.lock().await.len());
    }

    pub async fn subscribe_to_feed(
        &mut self,
        symbol: &str,
        underlying: &str,
        event_type: &[&str],
        instrument_type: InstrumentType,
        strike_price: Option<Decimal>,
    ) -> anyhow::Result<()> {
        let streamer_symbol = self.get_streamer_symbol(symbol, instrument_type).await?;
        info!(
            "Subscribing to mktdata events for symbol: {}",
            streamer_symbol
        );

        self.web_client
            .subscribe_to_symbol(&streamer_symbol, event_type)
            .await?;
        Self::stash_subscription(
            &mut self.events,
            symbol,
            underlying,
            &streamer_symbol,
            strike_price,
        )
        .await;
        Ok(())
    }

    pub async fn get_snapshot_by_symbol<'a, T>(&self, symbol: &str) -> Option<T::Event>
    where
        T: FeedEventExt + 'a,
        T::Event: std::fmt::Debug,
    {
        let reader = self.events.lock().await;
        let event = reader
            .iter()
            .find(|snapshot| snapshot.symbol.eq(symbol))
            .and_then(|snapshot| T::extract_event(snapshot));

        if let Some(event) = &event {
            info!("Mktdata symbol: {} Result {:?}", symbol, event);
        }

        event
    }

    pub async fn get_snapshot_by_underlying<'a, T>(&self, symbol: &str) -> Vec<Option<T::Event>>
    where
        T: FeedEventExt + 'a,
        T::Event: std::fmt::Debug,
    {
        let reader = self.events.lock().await;
        let events = reader
            .iter()
            .find(|snapshot| snapshot.symbol.eq(symbol))
            .map(|snapshot| T::extract_event(snapshot))
            .into_iter()
            .collect::<Vec<_>>();

        if !events.is_empty() {
            events
                .iter()
                .for_each(|event| info!("Mktdata symbol: {} Result {:?}", symbol, event));
        }

        events
    }

    async fn get_streamer_symbol(
        &self,
        symbol: &str,
        instrument_type: InstrumentType,
    ) -> Result<String> {
        let symbol = utf8_percent_encode(symbol, UTF8_ECODING).to_string();

        async fn streamer_symbol<Response>(web_client: &WebClient, endpoint: &str) -> Response
        where
            Response: for<'a> Deserialize<'a> + Serialize + fmt::Debug,
        {
            match web_client.get::<Response>(endpoint).await {
                anyhow::Result::Ok(response) => response,
                Err(e) => panic!("Error getting streamer symbol: {:?}", e),
            }
        }

        let streamer_symbol = match instrument_type {
            InstrumentType::Equity => {
                streamer_symbol::<Response<Equity>>(
                    &self.web_client,
                    &format!("instruments/equities/{}", symbol),
                )
                .await
                .data
                .streamer_symbol
            }
            InstrumentType::Future => {
                streamer_symbol::<Response<Future>>(
                    &self.web_client,
                    &format!("instruments/futures/{}", symbol),
                )
                .await
                .data
                .streamer_symbol
            }
            InstrumentType::EquityOption => {
                streamer_symbol::<Response<EquityOption>>(
                    &self.web_client,
                    &format!("instruments/equity-options/{}", symbol),
                )
                .await
                .data
                .streamer_symbol
            }
            InstrumentType::FutureOption => {
                streamer_symbol::<Response<FutureOption>>(
                    &self.web_client,
                    &format!("instruments/future-options/{}", symbol),
                )
                .await
                .data
                .streamer_symbol
            }
        };

        streamer_symbol.ok_or(anyhow!("Error getting streamer symbol: {}", symbol))
    }

    async fn stash_subscription(
        events: &mut Arc<Mutex<Vec<Snapshot>>>,
        symbol: &str,
        underlying: &str,
        streamer_symbol: &str,
        strike_price: Option<Decimal>,
    ) {
        let snapshot = Snapshot {
            symbol: symbol.to_string(),
            underlying: underlying.to_string(),
            streamer_symbol: streamer_symbol.to_string(),
            strike_price,
            last_update: Instant::now(),
            quote: None,
            greeks: None,
        };
        events.lock().await.push(snapshot);
    }
}
