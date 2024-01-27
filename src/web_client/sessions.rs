use anyhow::Result;
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use serde_json::to_string as to_json;
use sqlx::postgres::PgRow;
use sqlx::FromRow;
use sqlx::Row;
use std::sync::Arc;
use tokio::sync::broadcast::Sender;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::WebSocket;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;
use url::Url;

use super::http_client::HttpClient;
use super::websocket;
use super::websocket::WebSocketClient;
use super::ApiQuoteToken;

pub trait WsSession {
    fn url(&self) -> Url;
    fn to_ws(&self) -> &Sender<String>;
    fn is_alive(&self) -> bool;
    fn get_heartbeat_interval(&self) -> u64;
    fn last(&self) -> DateTime<Utc>;
    fn get_heart_beat_message(&self) -> String;
    fn handle_connect(&mut self, websocket_session_id: String);
    fn handle_heartbeat(&mut self);
    async fn handle_response<Session>(
        response: String,
        session: Arc<RwLock<Session>>,
        cancel_token: &CancellationToken,
    ) where
        Session: WsSession + std::marker::Send + std::marker::Sync + 'static;
}
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
pub struct AccountSession {
    url: Url,
    auth_token: String,
    session_id: String,
    last: DateTime<Utc>,
    to_ws: Sender<String>,
    is_alive: bool,
    heartbeat_interval: u64,
}

#[derive(FromRow, Clone, Default, Debug, Serialize, Deserialize)]
pub struct WsConnect {
    action: String,
    #[serde(rename = "value")]
    account_ids: Vec<String>,
    #[serde(rename = "auth-token")]
    pub auth_token: String,
}

impl WsSession for AccountSession {
    fn url(&self) -> Url {
        self.url.clone()
    }

    fn to_ws(&self) -> &Sender<String> {
        &self.to_ws
    }

    fn last(&self) -> DateTime<Utc> {
        self.last
    }

    fn is_alive(&self) -> bool {
        self.is_alive
    }

    fn get_heartbeat_interval(&self) -> u64 {
        self.heartbeat_interval
    }

    fn get_heart_beat_message(&self) -> String {
        let heartbeat = Heartbeat {
            action: "heartbeat".to_string(),
            auth_token: self.auth_token.clone(),
        };
        to_json(&heartbeat).unwrap()
    }

    fn handle_connect(&mut self, websocket_session_id: String) {
        self.session_id = websocket_session_id;
        self.is_alive = true;
    }

    fn handle_heartbeat(&mut self) {
        self.last = Utc::now();
    }

    async fn handle_response<Session>(
        response: String,
        session: Arc<RwLock<Session>>,
        cancel_token: &CancellationToken,
    ) where
        Session: WsSession + std::marker::Send + std::marker::Sync,
    {
        if let Ok(response) = serde_json::from_str::<Response>(&response) {
            if response.status.eq("ok") {
                let mut session = session.write().await;
                match response.action.as_str() {
                    "connect" => {
                        session.handle_connect(response.websocket_session_id.clone());
                    }
                    "heartbeat" => session.handle_heartbeat(),
                    _ => (),
                };
            } else {
                error!(
                    "Failed to connect to stream, action: {}, status: {}",
                    response.action, response.status
                );
                cancel_token.cancel()
            }
        };
    }
}

impl AccountSession {
    pub fn new(url: &str, to_ws: Sender<String>) -> Arc<RwLock<AccountSession>> {
        Arc::new(RwLock::new(AccountSession {
            url: Url::parse(url).unwrap(),
            session_id: String::default(),
            auth_token: String::default(),
            last: Utc::now(),
            to_ws,
            is_alive: false,
            heartbeat_interval: 30,
        }))
    }

    pub async fn startup(&mut self, account_id: &str, auth_token: &str) -> WsConnect {
        let connect = WsConnect {
            action: "connect".to_string(),
            account_ids: vec![account_id.to_string()],
            auth_token: auth_token.to_string(),
        };
        self.auth_token = auth_token.to_string();
        connect
    }

    pub fn get_auth_token(&self) -> &str {
        &self.auth_token
    }
}

#[derive(Clone, Debug)]
pub struct MktdataSession {
    api_quote_token: ApiQuoteToken,
    last: DateTime<Utc>,
    to_ws: Sender<String>,
    is_alive: bool,
    heartbeat_interval: u64,
}

impl MktdataSession {
    pub fn new(
        api_quote_token: ApiQuoteToken,
        to_ws: Sender<String>,
    ) -> Arc<RwLock<MktdataSession>> {
        Arc::new(RwLock::new(MktdataSession {
            api_quote_token,
            last: Utc::now(),
            to_ws,
            is_alive: false,
            heartbeat_interval: 60,
        }))
    }

    fn api_token(&self) -> Option<ApiQuoteToken> {
        Some(self.api_quote_token.clone())
    }
}

impl WsSession for MktdataSession {
    fn url(&self) -> Url {
        Url::parse(&self.api_quote_token.dxlink_url).unwrap()
    }

    fn to_ws(&self) -> &Sender<String> {
        &self.to_ws
    }

    fn last(&self) -> DateTime<Utc> {
        self.last
    }

    fn is_alive(&self) -> bool {
        self.is_alive
    }

    fn get_heartbeat_interval(&self) -> u64 {
        self.heartbeat_interval
    }

    fn get_heart_beat_message(&self) -> String {
        // let heartbeat = Heartbeat {
        //     action: "heartbeat".to_string(),
        //     // auth_token: self.auth_token.clone(),
        // };
        // to_json(&heartbeat).unwrap()
        "".to_string()
    }

    fn handle_connect(&mut self, websocket_session_id: String) {
        self.is_alive = true;
    }

    fn handle_heartbeat(&mut self) {
        self.last = Utc::now();
    }

    async fn handle_response<Session>(
        response: String,
        session: Arc<RwLock<Session>>,
        cancel_token: &CancellationToken,
    ) where
        Session: WsSession + std::marker::Send + std::marker::Sync + 'static,
    {
        if let Ok(response) = serde_json::from_str::<Response>(&response) {
            if response.status.eq("ok") {
                let mut session = session.write().await;
                match response.action.as_str() {
                    "connect" => {
                        session.handle_connect(response.websocket_session_id.clone());
                    }
                    "heartbeat" => session.handle_heartbeat(),
                    _ => (),
                };
            } else {
                error!(
                    "Failed to connect to stream, action: {}, status: {}",
                    response.action, response.status
                );
                cancel_token.cancel()
            }
        };
    }
}
