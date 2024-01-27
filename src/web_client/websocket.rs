use anyhow::Result;
use broadcast::error::RecvError;
use chrono::Utc;
use core::result::Result as CoreResult;
use futures_util::SinkExt;
use futures_util::StreamExt as _;
use native_tls::Protocol;
use native_tls::TlsConnector as NativeTlsConnector;
use serde::Deserialize;
use serde::Serialize;
use serde_json::to_string as to_json;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tokio::time::Duration;
use tokio_tungstenite::tungstenite::Error as WebSocketError;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::Connector;
use tokio_util::sync::CancellationToken;
use tracing::debug;
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

    async fn handle_socket_messages(
        message: Option<Result<Message, WebSocketError>>,
        session: Arc<RwLock<Session>>,
        cancel_token: &CancellationToken,
    ) where
        Session: WsSession + std::marker::Send + std::marker::Sync + 'static,
    {
        let _ = match message {
            Some(CoreResult::Ok(Message::Text(response))) => {
                info!("Receiving message {:?}", response);
                Session::handle_response(response, session, cancel_token).await;
                // let _ = from_ws.send(msg);
            }
            Some(CoreResult::Ok(Message::Pong(msg))) => {
                info!("Message received on websocket channel, msg: {:?}", msg);
            }
            Some(CoreResult::Ok(Message::Ping(msg))) => {
                info!("Message received on websocket channel, msg: {:?}", msg);
            }
            Some(CoreResult::Ok(Message::Binary(msg))) => {
                info!("Message received on websocket channel, msg: {:?}", msg);
            }
            Some(CoreResult::Ok(Message::Close(msg))) => {
                info!("Message received on websocket channel, msg: {:?}", msg);
            }
            Some(CoreResult::Ok(Message::Frame(msg))) => {
                info!("Message received on websocket channel, msg: {:?}", msg);
            }
            Some(Err(err)) => {
                error!("Error received on websocket channel, msg: {:?}", err)
                // Handle error
            }
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
        let heartbeat_interval = session.read().await.get_heartbeat_interval();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    msg = read.next() => {
                        Self::handle_socket_messages(msg, session.clone(), &cancel_token).await;
                    }
                    msg = to_ws.recv() => {
                        info!("Sending to ws {:?}", msg);
                        match msg {
                            Err(RecvError::Lagged(err)) => warn!("Publisher channel skipping a number of messages: {}", err),
                            Err(RecvError::Closed) => {
                                error!("Publisher channel closed");
                                cancel_token.cancel();
                            }
                            std::result::Result::Ok(val) => {
                                debug!("Sending payload {}", val);
                                let _ = write.send(Message::Text(val)).await;
                            }
                        };
                    }
                    _ = sleep(Duration::from_secs(heartbeat_interval)) => {
                        if Self::should_send_heartbeat(&session, &cancel_token).await {
                            let heartbeat = session.read().await.get_heart_beat_message();
                            let _ = write.send(Message::Text(heartbeat)).await;
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
        if session.last() + Duration::from_secs(60) < Utc::now() {
            error!("Heartbeat response not received in the last minute, forcing a restart");
            cancel_token.cancel();
            return false;
        }
        return true;
    }

    pub async fn send_message<Payload>(&self, payload: Payload) -> anyhow::Result<()>
    where
        Payload: Serialize + for<'a> Deserialize<'a>,
        Session: WsSession + std::marker::Send + std::marker::Sync + 'static,
    {
        let output = format!("to websocket sending payload: {}", to_json(&payload)?);
        info!("Sending to websocket: {}", output);
        match self.session.read().await.to_ws().send(to_json(&payload)?) {
            Err(err) => anyhow::bail!("Error sending payload to websocket stream, error: {}", err),
            _ => anyhow::Ok(()),
        }
    }
}
