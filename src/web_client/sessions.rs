use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use serde_json::to_string as to_json;
use sqlx::FromRow;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast::Sender;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::error;
use tracing::info;
use url::Url;

use crate::web_client::sessions::md_api::AddItem;

use self::md_api::AuthState;
use super::ApiQuoteToken;

pub trait WsSession {
    fn url(&self) -> Url;
    fn token(&self) -> String;
    fn to_ws(&self) -> &Sender<String>;
    fn is_alive(&self) -> bool;
    fn heartbeat_interval(&self) -> u64;
    fn last_received(&self) -> DateTime<Utc>;
    fn last_sent(&self) -> DateTime<Utc>;
    fn update_last_sent(&mut self);
    fn get_heart_beat_message(&self) -> String;
    // fn handle_connect(&mut self, websocket_session_id: String);
    fn handle_heartbeat(&mut self);
    fn handle_response<Session>(&mut self, response: String, cancel_token: CancellationToken)
    where
        Session: WsSession + std::marker::Send + std::marker::Sync + 'static;
}

pub(crate) mod acc_api {
    use super::*;
    #[derive(Clone, Default, Debug, Serialize, Deserialize)]
    pub struct AccHeartbeat {
        pub action: String,
        #[serde(rename = "auth-token")]
        pub auth_token: String,
    }

    #[derive(Clone, Default, Debug, Deserialize)]
    pub struct Response {
        pub status: String,
        pub action: String,
        #[serde(rename = "web-socket-session-id")]
        pub websocket_session_id: String,
        pub value: Option<Vec<String>>,
        #[serde(rename = "request-id")]
        pub request_id: u8,
    }

    #[derive(FromRow, Clone, Default, Debug, Serialize, Deserialize)]
    pub struct Connect {
        pub action: String,
        #[serde(rename = "value")]
        pub account_ids: Vec<String>,
        #[serde(rename = "auth-token")]
        pub auth_token: String,
    }
    #[derive(Debug, Serialize, Deserialize)]
    pub struct Payload {
        #[serde(rename = "type")]
        pub msg_type: String,
        pub data: String,
        pub timestamp: u32,
    }
}
#[derive(Clone, Debug)]
pub struct AccountSession {
    url: Url,
    auth_token: String,
    session_id: String,
    last_received: DateTime<Utc>,
    last_sent: DateTime<Utc>,
    to_ws: Sender<String>,
    to_app: Sender<String>,
    is_alive: bool,
    heartbeat_interval: u64,
}

impl AccountSession {
    pub fn new(
        url: &str,
        to_ws: Sender<String>,
        to_app: Sender<String>,
    ) -> Arc<RwLock<AccountSession>> {
        Arc::new(RwLock::new(AccountSession {
            url: Url::parse(url).unwrap(),
            session_id: String::default(),
            auth_token: String::default(),
            last_received: Utc::now(),
            last_sent: Utc::now(),
            to_ws,
            to_app,
            is_alive: false,
            heartbeat_interval: 60,
        }))
    }

    pub async fn startup(&mut self, account_id: &str, auth_token: &str) -> acc_api::Connect {
        let connect = acc_api::Connect {
            action: "connect".to_string(),
            account_ids: vec![account_id.to_string()],
            auth_token: auth_token.to_string(),
        };
        self.auth_token = auth_token.to_string();
        connect
    }

    fn handle_connect(&mut self, websocket_session_id: String) {
        self.session_id = websocket_session_id;
        self.is_alive = true;
    }
}

impl WsSession for AccountSession {
    fn url(&self) -> Url {
        self.url.clone()
    }

    fn token(&self) -> String {
        self.session_id.clone()
    }

    fn to_ws(&self) -> &Sender<String> {
        &self.to_ws
    }

    fn last_received(&self) -> DateTime<Utc> {
        self.last_received
    }

    fn last_sent(&self) -> DateTime<Utc> {
        self.last_sent
    }

    fn is_alive(&self) -> bool {
        self.is_alive
    }

    fn heartbeat_interval(&self) -> u64 {
        self.heartbeat_interval
    }

    fn get_heart_beat_message(&self) -> String {
        let heartbeat = acc_api::AccHeartbeat {
            action: "heartbeat".to_string(),
            auth_token: self.auth_token.clone(),
        };
        to_json(&heartbeat).unwrap()
    }

    fn update_last_sent(&mut self) {
        self.last_sent = Utc::now();
    }

    fn handle_heartbeat(&mut self) {
        self.last_received = Utc::now();
    }

    fn handle_response<Session>(&mut self, response: String, cancel_token: CancellationToken)
    where
        Session: WsSession + std::marker::Send + std::marker::Sync + 'static,
    {
        debug!("Response on account session, msg: {}", response);
        if let Ok(response) = serde_json::from_str::<acc_api::Response>(&response) {
            if response.status.eq("ok") {
                match response.action.as_str() {
                    "connect" => {
                        self.handle_connect(response.websocket_session_id.clone());
                    }
                    "heartbeat" => self.handle_heartbeat(),
                    _ => info!("Here, {:?}", response),
                };
            } else {
                error!(
                    "Failed to connect to stream, action: {}, status: {}",
                    response.action, response.status
                );
                cancel_token.cancel()
            }
        } else {
            let _ = self.to_app.send(response).unwrap();
        }
    }
}

pub(crate) mod md_api {

    use super::*;

    #[derive(Debug, Serialize, Deserialize)]
    pub struct ChannelRequest {
        #[serde(rename = "type")]
        msg_type: String,
        pub channel: u32,
        pub service: String,
        pub parameters: Parameters,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct FeedSubscription {
        #[serde(rename = "type")]
        pub msg_type: String,
        pub channel: i64,
        pub add: Vec<AddItem>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct ChannelCancel {
        #[serde(rename = "type")]
        pub msg_type: String,
        pub channel: i64,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct ClientChannelRequest {
        #[serde(rename = "type")]
        pub msg_type: String,
        pub service: String,
        pub parameters: Parameters,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct ClientFeedSubscription {
        #[serde(rename = "type")]
        pub msg_type: String,
        pub channel: u64,
        pub add: Vec<AddItem>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Parameters {
        pub contract: String,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct AddItem {
        pub symbol: String,
        #[serde(rename = "type")]
        pub msg_type: String,
        pub from_time: Option<i64>,
    }

    #[derive(Clone, Default, Debug, Serialize, Deserialize)]
    pub struct Connect {
        #[serde(rename = "type")]
        pub msg_type: String,
        pub channel: u64,
        #[serde(rename = "keepaliveTimeout")]
        pub keepalive_timeout: u64,
        #[serde(rename = "acceptKeepaliveTimeout")]
        pub accept_keepalive_timeout: u64,
        pub version: String,
    }

    #[derive(Clone, Default, Debug, Serialize, Deserialize)]
    pub struct Heartbeat {
        #[serde(rename = "type")]
        pub msg_type: String,
        pub channel: u64,
    }

    #[derive(Clone, Default, Debug, Serialize, Deserialize)]
    pub struct Auth {
        #[serde(rename = "type")]
        pub msg_type: String,
        pub token: String,
        pub channel: u64,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct AuthState {
        #[serde(rename = "type")]
        pub msg_type: String,
        pub state: String,
        #[serde(rename = "userId")]
        pub user_id: Option<String>,
        pub channel: u64,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Channel {
        #[serde(rename = "type")]
        pub msg_type: String,
        pub channel: i64,
        pub service: String,
        pub parameters: HashMap<String, String>,
    }
}

#[derive(Clone, Debug)]
pub struct MktdataSession {
    api_quote_token: ApiQuoteToken,
    last_received: DateTime<Utc>,
    last_sent: DateTime<Utc>,
    to_ws: Sender<String>,
    to_app: Sender<String>,
    channels: Vec<String>,
    is_alive: bool,
    heartbeat_interval: u64,
}

impl MktdataSession {
    pub fn new(
        api_quote_token: ApiQuoteToken,
        to_ws: Sender<String>,
        to_app: Sender<String>,
    ) -> Arc<RwLock<MktdataSession>> {
        Arc::new(RwLock::new(MktdataSession {
            api_quote_token,
            last_received: Utc::now(),
            last_sent: Utc::now(),
            to_ws,
            to_app,
            channels: Vec::default(),
            is_alive: false,
            heartbeat_interval: 30,
        }))
    }

    pub async fn startup(&mut self) -> md_api::Connect {
        md_api::Connect {
            msg_type: "SETUP".to_string(),
            channel: 0,
            keepalive_timeout: self.heartbeat_interval,
            accept_keepalive_timeout: self.heartbeat_interval,
            version: "0.1".to_string(),
        }
    }

    fn handle_auth(&self, auth: AuthState) -> Option<md_api::Auth> {
        match auth.state.as_str() {
            "UNAUTHORIZED" => Some(md_api::Auth {
                msg_type: "AUTH".to_string(),
                channel: 0,
                token: self.api_quote_token.token.clone(),
            }),
            "AUTHORIZED" => {
                info!("Connection authorized, channel: {}", 0);
                None
            }
            _ => None,
        }
    }

    pub fn subscribe(&mut self, symbol: &str) -> md_api::Channel {
        let mut parameters = HashMap::new();
        parameters.insert("contract".to_string(), "AUTO".to_string());
        let message = md_api::Channel {
            msg_type: "CHANNEL_REQUEST".to_string(),
            channel: self.channels.len() as i64,
            service: "FEED".to_string(),
            parameters,
        };
        self.channels.push(symbol.to_string());
        message
    }

    fn handle_connect(&mut self) {
        self.is_alive = true;
    }
}

impl WsSession for MktdataSession {
    fn url(&self) -> Url {
        Url::parse(&self.api_quote_token.dxlink_url).unwrap()
    }

    fn token(&self) -> String {
        self.api_quote_token.token.clone()
    }

    fn to_ws(&self) -> &Sender<String> {
        &self.to_ws
    }

    fn last_received(&self) -> DateTime<Utc> {
        self.last_received
    }

    fn last_sent(&self) -> DateTime<Utc> {
        self.last_sent
    }

    fn update_last_sent(&mut self) {
        self.last_sent = Utc::now();
    }

    fn is_alive(&self) -> bool {
        self.is_alive
    }

    fn heartbeat_interval(&self) -> u64 {
        self.heartbeat_interval
    }

    fn get_heart_beat_message(&self) -> String {
        let heartbeat = md_api::Heartbeat {
            msg_type: "KEEPALIVE".to_string(),
            channel: 0,
        };
        to_json(&heartbeat).unwrap()
    }

    fn handle_heartbeat(&mut self) {
        self.last_received = Utc::now();
    }

    fn handle_response<Session>(&mut self, response: String, _cancel_token: CancellationToken)
    where
        Session: WsSession + std::marker::Send + std::marker::Sync + 'static,
    {
        if let Ok(response) = serde_json::from_str::<md_api::AuthState>(&response) {
            info!("connection response auth state: {:?}", response);
            if let Some(auth) = self.handle_auth(response) {
                self.handle_connect();
                info!("sending auth request: {:?}", auth);
                let _ = self.to_ws.send(to_json(&auth).unwrap()).unwrap();
            }
        } else if let Ok(response) = serde_json::from_str::<md_api::Heartbeat>(&response) {
            info!("heartbeat response {:?}", response);
            self.handle_heartbeat();
        } else if let Ok(response) = serde_json::from_str::<md_api::Channel>(&response) {
            info!("channel response {:?}", response);
            if response.msg_type.eq("CHANNEL_OPENED") {
                let channel = response.channel;
                let symbol = self.channels[channel as usize].to_string();
                let subscription = md_api::FeedSubscription {
                    msg_type: "FEED_SUBSCRIPTION".to_string(),
                    channel,
                    add: vec![AddItem {
                        symbol,
                        msg_type: "Candle".to_string(),
                        from_time: None,
                    }],
                };
                let _ = self.to_ws.send(to_json(&subscription).unwrap()).unwrap();
            }
        } else {
            let _ = self.to_app.send(response);
        }
        // match serde_json::from_str::<md_api::Heartbeat>(&response) {
        //     Ok(response) => {
        //         self.handle_heartbeat();
        //         info!("Info response {:?}", response);
        //     }
        //     Err(err) => error!("Failed to parse, error: {}", err),
        // }
    }
}
