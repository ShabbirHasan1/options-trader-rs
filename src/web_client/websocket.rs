use anyhow::Result;
use broadcast::error::RecvError;
use chrono::DateTime;
use chrono::Utc;
use core::result::Result as CoreResult;
use futures_util::SinkExt;
use futures_util::StreamExt as _;
use native_tls::Protocol;
use native_tls::TlsConnector as NativeTlsConnector;
use serde::Deserialize;
use serde::Serialize;
use serde_json::from_str as json_from_str;
use serde_json::to_string as to_json;
use std::sync::Arc;
use std::sync::RwLock;
use tokio::sync::broadcast;
use tokio::sync::broadcast::Sender;
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
use url::Url;

use super::WsConnect;

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
struct Heartbeat {
    action: String,
    #[serde(rename = "auth-token")]
    auth_token: String,
}

#[derive(Clone, Default, Debug, Deserialize)]
struct Response {
    status: String,
    action: String,
    #[serde(rename = "web-socket-session-id")]
    websocket_session_id: String,
    value: Option<Vec<String>>,
    #[serde(rename = "request-id")]
    request_id: u8,
}

#[derive(Clone, Debug)]
pub struct WebSocketClient<Session> {
    session: Arc<RwLock<Session>>,
    to_app: Sender<String>,
    cancel_token: CancellationToken,
}

impl<Session> WebSocketClient<Session> {
    pub async fn new(
        session: Arc<RwLock<Session>>,
        to_app: Sender<String>,
        cancel_token: CancellationToken,
    ) -> Result<Self> {
        // WebSocket server URL
        info!("Creating websocket with target host: {}", url);
        Ok(Self {
            session,
            to_app,
            cancel_token,
        })
    }

    pub async fn startup(&mut self, connect: WsConnect) -> anyhow::Result<()> {
        self.session.write().unwrap().auth_token() = connect.auth_token.clone();
        self.send_message::<WsConnect>(connect).await
    }

    fn handle_response(
        response: Response,
        session: &Arc<RwLock<Session>>,
        cancel_token: &CancellationToken,
    ) {
        if response.status.eq("ok") {
            match session.write() {
                Ok(mut session) => {
                    match response.action.as_str() {
                        "connect" => {
                            session.session_id() = response.websocket_session_id;
                            session.is_alive() = true;
                        }
                        "heartbeat" => session.last() = Utc::now(),
                        _ => (),
                    };
                }
                Err(err) => {
                    error!("Unable to write to session, error: {}", err);
                    cancel_token.cancel();
                }
            }
        } else {
            error!(
                "Failed to connect to stream, action: {}, status: {}",
                response.action, response.status
            );
            cancel_token.cancel()
        }
    }

    fn handle_socket_messages(
        message: Option<Result<Message, WebSocketError>>,
        from_ws: &Sender<String>,
        session: &Arc<RwLock<Session>>,
        cancel_token: &CancellationToken,
    ) {
        let _ = match message {
            Some(CoreResult::Ok(Message::Text(msg))) => {
                info!("Receiving message {}", msg);
                if let Ok(response) = serde_json::from_str::<Response>(&msg) {
                    Self::handle_response(response, session, cancel_token);
                    return;
                };
                let _ = match serde_json::from_str::<Heartbeat>(&msg) {
                    Ok(response) => {
                        session.write().unwrap().last() = Utc::now();
                        return;
                    }
                    Err(err) => {
                        error!("Error received deserialising message, error: {}", err);
                    }
                };
                info!("Message received on websocket channel, msg: {:?}", msg);
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

    pub async fn subscribe_to_events(&self) -> Result<()> {
        let tls_connector = NativeTlsConnector::builder()
            .min_protocol_version(Some(Protocol::Tlsv12))
            .build()
            .expect("Failed to build tlsconnector");

        let (stream, response) = tokio_tungstenite::connect_async_tls_with_config(
            &self.url,
            None,
            false,
            Some(Connector::NativeTls(tls_connector)),
        )
        .await?;

        dbg!("Websocket connect response: {:?}", response);

        let (mut write, mut read) = stream.split();
        let from_ws = self.to_app.clone();
        let cancel_token = self.cancel_token.clone();
        let session = Arc::clone(&self.session);
        let mut to_ws = session.read().unwrap().to_ws().subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    msg = read.next() => {
                        Self::handle_socket_messages(msg, &from_ws, &session, &cancel_token);
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
                    _ = sleep(Duration::from_secs(30)) => {
                        if Self::should_send_heartbeat(&session, &cancel_token) {
                            let heartbeat = Heartbeat{action: "heartbeat".to_string(), auth_token: session.read().unwrap().auth_token.clone() };
                            let _ = write.send(Message::Text(to_json(&heartbeat).unwrap())).await;
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

    fn should_send_heartbeat(
        session: &Arc<RwLock<Session>>,
        cancel_token: &CancellationToken,
    ) -> bool {
        match session.read() {
            Ok(session) => {
                if !session.is_alive() {
                    return false;
                }
                if session.last() + Duration::from_secs(60) < Utc::now() {
                    error!("Heartbeat response not received in the last minute, forcing a restart");
                    cancel_token.cancel();
                    return false;
                }
                true
            }
            Err(err) => {
                error!("Unable to write to session, error: {}", err);
                cancel_token.cancel();
                false
            }
        }
    }

    pub async fn send_message<Payload>(&mut self, payload: Payload) -> anyhow::Result<()>
    where
        Payload: Serialize + for<'a> Deserialize<'a>,
    {
        let output = format!("to websocket sending payload: {}", to_json(&payload)?);
        info!("Sending to websocket: {}", output);
        match self.session.read().unwrap().to_ws.send(to_json(&payload)?) {
            Err(err) => anyhow::bail!("Error sending payload to websocket stream, error: {}", err),
            _ => anyhow::Ok(()),
        }
    }
}
