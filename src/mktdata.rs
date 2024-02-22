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
    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Event {}
}

pub(crate) struct MktData {
    web_client: Arc<WebClient>,
    event: HashMap<String, tt_api::Event>,
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
        self.web_client.subscribe_to_symbol(symbol).await
    }

    pub fn get_snapshot(&self, symbol: &str) -> tt_api::Event {
        self.event[symbol].clone()
    }

    fn handle_msg(msg: String, _cancel_token: &CancellationToken) {
        // if let Ok(msg) = serde_json::from_str::<tt_api::Event>(&msg) {
        //     info!("Last mktdata message received, msg: {:?}", msg);
        // }
    }
}
