use anyhow::Result;
use broadcast::error::RecvError;
use chrono::Utc;
use futures_util::SinkExt;
use futures_util::StreamExt as _;
use native_tls::Protocol;
use native_tls::TlsConnector as NativeTlsConnector;
use serde::Deserialize;
use serde::Serialize;
use serde_json::to_string as to_json;
use std::sync::Arc;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;

use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tokio::time::Duration;
use tokio_tungstenite::tungstenite::Error as WebSocketError;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::Connector;
use tokio_util::sync::CancellationToken;

use tracing::error;
use tracing::info;
use tracing::warn;

use super::sessions::WsSession;

#[derive(Clone, Debug)]
pub struct WebSocketClient<Session> {
    session: Arc<RwLock<Session>>,
    cancel_token: CancellationToken,
}

impl<Session> WebSocketClient<Session> {
    pub fn new(session: Arc<RwLock<Session>>, cancel_token: CancellationToken) -> Result<Self> {
        Ok(Self {
            session,
            cancel_token,
        })
    }

    pub fn get_session(&self) -> Arc<RwLock<Session>> {
        self.session.clone()
    }

    async fn handle_socket_messages(
        message: Option<Result<Message, WebSocketError>>,
        session: Arc<RwLock<Session>>,
        cancel_token: CancellationToken,
    ) where
        Session: WsSession + std::marker::Send + std::marker::Sync + 'static,
    {
        match message {
            Some(var) => match var {
                Ok(Message::Text(response)) => {
                    session
                        .write()
                        .await
                        .handle_response::<Session>(response, cancel_token);
                }
                Ok(Message::Close(Some(CloseFrame { code, reason }))) => {
                    info!("Exit code: {}, reason: {}", code, reason)
                }
                Ok(var) => info!("unknown type: {:?}", var),
                Err(err) => error!("Error: {}", err),
            },
            None => {
                info!("Stream closed, cancelling session on client");
                cancel_token.cancel();
            }
        };
    }

    pub async fn subscribe_to_events(&self) -> Result<()>
    where
        Session: WsSession + std::marker::Send + std::marker::Sync + 'static,
    {
        let tls_connector = NativeTlsConnector::builder()
            .min_protocol_version(Some(Protocol::Tlsv12))
            .build()
            .expect("Failed to build tlsconnector");

        let (stream, response) = tokio_tungstenite::connect_async_tls_with_config(
            self.session.read().await.url(),
            None,
            false,
            Some(Connector::NativeTls(tls_connector)),
        )
        .await?;

        dbg!("Websocket connect response: {:?}", response);

        let (mut write, mut read) = stream.split();
        let cancel_token = self.cancel_token.clone();
        let session = Arc::clone(&self.session);
        let mut to_ws = session.read().await.to_ws().subscribe();
        let heartbeat_interval = session.read().await.heartbeat_interval();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    msg = read.next() => {
                        Self::handle_socket_messages(msg, session.clone(), cancel_token.clone()).await;
                    }
                    msg = to_ws.recv() => {
                        match msg {
                            Err(RecvError::Lagged(err)) => warn!("Publisher channel skipping a number of messages: {}", err),
                            Err(RecvError::Closed) => {
                                error!("Publisher channel closed");
                                cancel_token.cancel();
                            }
                            std::result::Result::Ok(val) => {
                                info!("Sending payload {}", val);
                                let _ = write.send(Message::Text(val)).await;
                            }
                        };
                    }
                    _ = sleep(Duration::from_secs(1)) => {
                        if Self::should_send_heartbeat(heartbeat_interval, &session, &cancel_token).await {
                            let heartbeat = session.read().await.get_heart_beat_message();
                            if write.send(Message::Text(heartbeat)).await.is_ok() {
                                session.write().await.update_last_sent();
                            }
                        }
                    }
                    _ = cancel_token.cancelled() => {
                        break;
                    }
                }
            }
        });
        Ok(())
    }

    async fn should_send_heartbeat(
        interval: u64,
        session: &Arc<RwLock<Session>>,
        cancel_token: &CancellationToken,
    ) -> bool
    where
        Session: WsSession,
    {
        let session = session.read().await;
        if !session.is_alive() {
            return false;
        }
        let now = Utc::now();
        if session.last_received() + Duration::from_millis(interval * 1200) < now {
            error!("Heartbeat response not received in the last minute, forcing a restart");
            cancel_token.cancel();
            false
        } else {
            session.last_sent() + Duration::from_secs(interval - 5) <= now
        }
    }

    pub async fn send_message<Payload>(&self, payload: Payload) -> anyhow::Result<()>
    where
        Payload: Serialize + for<'a> Deserialize<'a>,
        Session: WsSession + std::marker::Send + std::marker::Sync + 'static,
    {
        info!("Sending to websocket: {}", to_json(&payload)?);
        match self.session.read().await.to_ws().send(to_json(&payload)?) {
            Err(err) => anyhow::bail!("Error sending payload to websocket stream, error: {}", err),
            _ => anyhow::Ok(()),
        }
    }
}
