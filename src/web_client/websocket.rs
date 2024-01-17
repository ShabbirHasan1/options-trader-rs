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
use std::ops::Deref;
use std::ops::DerefMut;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::broadcast::Sender;
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
use url::Url;

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
struct Heartbeat {
    action: String,
    session_id: String,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
struct AuthResponse {
    status: String,
    action: String,
    websocket_session_id: String,
    value: String,
    request_id: String,
}

#[derive(Clone, Debug)]
struct Session {
    session_id: String,
    last: DateTime<Utc>,
    to_ws: Sender<String>,
    is_alive: bool,
}

#[derive(Clone, Debug)]
pub struct WebSocketClient {
    url: Url,
    session: Arc<Session>,
    from_ws: Sender<String>,
    cancel_token: CancellationToken,
}

impl WebSocketClient {
    pub async fn new(
        url: &str,
        from_ws: Sender<String>,
        cancel_token: CancellationToken,
    ) -> Result<Self> {
        // WebSocket server URL
        info!("Creating websocket with target host: {}", url);
        let (to_ws, _) = broadcast::channel::<String>(100);
        let session = Arc::new(Session {
            session_id: String::default(),
            last: Utc::now(),
            to_ws,
            is_alive: false,
        });

        Ok(Self {
            url: Url::parse(url)?,
            session,
            from_ws,
            cancel_token,
        })
    }

    fn handle_socket_messages(
        message: Option<Result<Message, WebSocketError>>,
        from_ws: &Sender<String>,
        session: &mut Arc<Session>,
        cancel_token: &CancellationToken,
    ) {
        match message {
            Some(CoreResult::Ok(Message::Text(msg))) => {
                let _ = match serde_json::from_str::<AuthResponse>(&msg) {
                    Ok(response) => {
                        if response.action.eq("connect") && response.status.eq("ok") {
                            session.session_id = response.websocket_session_id;
                            session.is_alive = true;
                        } else {
                            error!(
                                "Failed to connect to stream, action: {}, status: {}",
                                response.action, response.status
                            );
                            cancel_token.cancel()
                        }
                        return;
                    }
                    _ => (),
                };
                info!("Message received on websocket channel, msg: {:?}", msg);
                let _ = from_ws.send(msg);
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

        let (stream, _response) = tokio_tungstenite::connect_async_tls_with_config(
            &self.url,
            None,
            false,
            Some(Connector::NativeTls(tls_connector)),
        )
        .await?;

        let (mut write, mut read) = stream.split();
        let from_ws = self.from_ws.clone();
        let cancel_token = self.cancel_token.clone();
        let session = Arc::clone(&mut self.session);
        tokio::spawn(async move {
            let read_ws = from_ws.subscribe();
            let mut to_ws = session.to_ws.subscribe();
            loop {
                tokio::select! {
                    msg = read.next() => {
                        Self::handle_socket_messages(msg, &from_ws, &session, &cancel_token);
                    }
                    msg = to_ws.recv() => {
                        match msg {
                            Err(RecvError::Lagged(err)) => warn!("Publisher channel skipping a number of messages: {}", err),
                            Err(RecvError::Closed) => {
                                error!("Publisher channel closed");
                                cancel_token.cancel();
                            }
                            std::result::Result::Ok(val) => {
                                let _ = write.send(Message::Text(val)).await;
                            }
                        };
                    }
                    _ = sleep(Duration::from_secs(30)) => {
                        if Self::should_send_heartbeat(&session, &cancel_token) {
                            let heartbeat = Heartbeat{action: "heartbeat".to_string(), session_id: session.session_id.clone() };
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

    fn should_send_heartbeat(session: &Arc<Session>, cancel_token: &CancellationToken) -> bool {
        if !session.is_alive {
            return false;
        }
        if session.last + Duration::from_secs(60) < Utc::now() {
            error!("Heartbeat response not received in the last minute, forcing a restart");
            cancel_token.cancel();
            return false;
        }
        return true;
    }

    pub async fn send_message<Payload>(&mut self, payload: Payload) -> anyhow::Result<()>
    where
        Payload: Serialize + for<'a> Deserialize<'a>,
    {
        let output = format!("to websocket sending payload: {}", to_json(&payload)?);
        info!("Sending to websocket: {}", output);
        match self.session.to_ws.send(to_json(&payload)?) {
            Err(err) => anyhow::bail!("Error sending payload to websocket stream, error: {}", err),
            _ => anyhow::Ok(()),
        }
    }
}
