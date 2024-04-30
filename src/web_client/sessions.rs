use anyhow::bail;

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

use self::md_api::FeedData;
use self::md_api::Header;
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
            heartbeat_interval: 30,
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
        debug!(
            "[Account Session] response on account session, msg: {}",
            response
        );
        if let serde_json::Result::Ok(response) =
            serde_json::from_str::<acc_api::Response>(&response)
        {
            if response.status.eq("ok") {
                match response.action.as_str() {
                    "connect" => {
                        info!("[Account Session] connect response {:?}", response);
                        self.handle_connect(response.websocket_session_id.clone());
                    }
                    "heartbeat" => {
                        info!("[Account Session] heartbeat response {:?}", response);
                        self.handle_heartbeat();
                    }
                    _ => info!("Here, {:?}", response),
                };
            } else {
                error!(
                    "[Account Session] Failed to connect to stream, action: {}, status: {}",
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

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Header {
        #[serde(rename = "type")]
        pub msg_type: String,
        pub channel: u64,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct ChannelRequest {
        #[serde(flatten)]
        pub msg: Header,
        pub service: String,
        pub parameters: Parameters,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct FeedSubscription {
        #[serde(flatten)]
        pub msg: Header,
        pub add: Vec<AddItem>,
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
        #[serde(flatten)]
        pub msg: Header,
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
        // #[serde(skip_serializing_if = "Option::is_none")]
        // pub from_time: Option<i64>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct FeedSetup {
        #[serde(flatten)]
        pub msg: Header,
        #[serde(rename = "acceptAggregationPeriod")]
        pub accept_aggregation_period: Option<i64>,
        #[serde(rename = "acceptDataFormat")]
        pub accept_data_format: Option<String>,
        #[serde(rename = "acceptEventFields")]
        pub accept_event_fields: Option<AcceptEventFields>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct AcceptEventFields {
        #[serde(rename = "Quote")]
        pub quote: Option<Vec<String>>,
        #[serde(rename = "Candle")]
        pub candle: Option<Vec<String>>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Connect {
        #[serde(flatten)]
        pub msg: Header,
        #[serde(rename = "keepaliveTimeout")]
        pub keepalive_timeout: u64,
        #[serde(rename = "acceptKeepaliveTimeout")]
        pub accept_keepalive_timeout: u64,
        pub version: String,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Candle {
        #[serde(rename = "Candle")]
        pub candle: Option<Vec<String>>,
    }

    // #[derive(Clone, Debug, Serialize, Deserialize)]
    // pub struct Data {
    //     pub data: Vec,
    // }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct FeedData {
        #[serde(flatten)]
        pub msg: Header,
        #[serde(flatten)]
        pub data: Option<String>,
        pub state: Option<String>,
        pub error: Option<String>,
        pub message: Option<String>,
        #[serde(rename = "eventFields")]
        pub event_fields: Option<Candle>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Auth {
        #[serde(flatten)]
        pub msg: Header,
        pub token: String,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct AuthState {
        pub msg: Header,
        pub state: String,
        #[serde(rename = "userId")]
        pub user_id: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Channel {
        #[serde(flatten)]
        pub msg: Header,
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
    waiting_on_subscription: Vec<String>,
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
            waiting_on_subscription: Vec::default(),
            is_alive: false,
            heartbeat_interval: 55,
        }))
    }

    pub async fn startup(&mut self) -> md_api::Connect {
        md_api::Connect {
            msg: Header {
                msg_type: "SETUP".to_string(),
                channel: 0,
            },
            keepalive_timeout: self.heartbeat_interval,
            accept_keepalive_timeout: self.heartbeat_interval,
            version: "0.1".to_string(),
        }
    }

    fn handle_auth(&self, payload: FeedData) -> anyhow::Result<()> {
        match payload.state.unwrap().as_str() {
            "UNAUTHORIZED" => {
                let request = md_api::Auth {
                    msg: Header {
                        msg_type: "AUTH".to_string(),
                        channel: 0,
                    },
                    token: self.api_quote_token.token.clone(),
                };
                match self.to_ws.send(to_json(&request).unwrap()) {
                    Err(err) => bail!("Failed to subscribe request: {:?}, error: {}", request, err),
                    _ => anyhow::Ok(()),
                }
            }
            "AUTHORIZED" => {
                info!("Connection authorized, channel: {}", 0);

                let mut parameters = HashMap::new();
                parameters.insert("contract".to_string(), "AUTO".to_string());
                let request = md_api::Channel {
                    msg: Header {
                        msg_type: "CHANNEL_REQUEST".to_string(),
                        channel: 1_u64,
                    },
                    service: "FEED".to_string(),
                    parameters,
                };
                match self.to_ws.send(to_json(&request).unwrap()) {
                    Err(err) => bail!("Failed to subscribe request: {:?}, error: {}", request, err),
                    _ => anyhow::Ok(()),
                }
            }
            _ => bail!("Unknown auth"),
        }
    }

    pub fn subscribe(&mut self, symbol: Option<&str>, event_type: Vec<&str>) -> anyhow::Result<()> {
        if let Some(symbol) = symbol {
            self.waiting_on_subscription.push(symbol.to_string());
        }
        if !self.is_alive || self.waiting_on_subscription.is_empty() {
            return anyhow::Ok(());
        }
        let mut subscriptions = Vec::new();
        self.waiting_on_subscription.iter().for_each(|symbol| {
            event_type.iter().for_each(|event| {
                subscriptions.push(AddItem {
                    symbol: symbol.clone(),
                    msg_type: event.to_string(),
                })
            })
        });
        let subscription = md_api::FeedSubscription {
            msg: Header {
                msg_type: "FEED_SUBSCRIPTION".to_string(),
                channel: 1_u64,
            },
            add: subscriptions,
        };
        info!("Subscription looks like {:?}", &subscription);
        match self.to_ws.send(to_json(&subscription).unwrap()) {
            Err(err) => bail!(
                "Failed to subscribe request: {:?}, error: {}",
                subscription,
                err
            ),
            _ => {
                self.waiting_on_subscription.clear();
                anyhow::Ok(())
            }
        }
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
        let heartbeat = Header {
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
        info!("response {}", response);
        if let serde_json::Result::Ok(payload) = serde_json::from_str::<md_api::FeedData>(&response)
        {
            match payload.msg.msg_type.as_str() {
                "KEEPALIVE" => {
                    info!("[MktData Session] heartbeat {:?}", payload);
                    self.handle_heartbeat();
                }
                "AUTH_STATE" => {
                    info!(
                        "[MktData Session] connection response auth state: {:?}",
                        payload
                    );
                    let _ = self.handle_auth(payload);
                }
                "CHANNEL_OPENED" => {
                    info!("[MktData Session] Channel session {:?}", payload);
                    self.handle_connect();
                }
                "FEED_CONFIG" => {
                    if let Some(_config) = payload.event_fields.as_ref() {
                        info!("{:?} ", payload.clone());
                        info!("{:?} ", response.clone());
                        // let mut update = config.candle.as_ref().unwrap().clone();
                        // update.push("greeks".to_string());
                        // let response = FeedSetup {
                        //     msg: Header {
                        //         msg_type: "FEED_SETUP".to_string(),
                        //         channel: 1_u64,
                        //     },
                        //     accept_aggregation_period: Some(10),
                        //     accept_data_format: Some("FULL".to_string()),
                        //     accept_event_fields: Some(md_api::AcceptEventFields {
                        //         quote: None,
                        //         candle: Some(update),
                        //     }),
                        // };
                        // let _ = self.to_app.send(to_json(&response).unwrap());
                    }
                }
                "FEED_DATA" => {
                    let _ = self.to_app.send(response);
                }
                "ERROR" => {
                    info!("{:?} ", payload);
                }
                _ => info!("Unknown? {:?} ", payload),
            };
            // } else if let Ok(response) = serde_json::from_str::<md_api::Channel>(&response) {
            //     info!("[MktData Session] channel response {:?}", response);
            // } else {
            //     info!("Mktdata? {} ", response);
            //     let _ = self.to_app.send(response);
            // }
        } else {
            info!("Mktdata end? {:?} ", response);
        }
    }
}
