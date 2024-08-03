use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::connectivity::web_client::WebClient;
use crate::tt_api::account::*;
use crate::tt_api::sessions::Payload;

pub struct Account {}

impl Account {
    pub fn new(web_client: Arc<WebClient>, cancel_token: CancellationToken) -> Self {
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
        Self {}
    }

    fn handle_msg(msg: String, _cancel_token: &CancellationToken) {
        if let Ok(payload) = serde_json::from_str::<Payload>(&msg) {
            if payload.msg_type.ne("AccountBalance") {
                return;
            }
            if let Ok(msg) = serde_json::from_str::<AccountBalance>(&payload.data) {
                info!("Last account balance message received, msg: {:?}", msg);
            }
        }
    }
}
