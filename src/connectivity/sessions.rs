use anyhow::bail;

use chrono::DateTime;
use chrono::Utc;
use serde_json::to_string as to_json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast::Sender;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::error;
use tracing::info;
use url::Url;

use crate::platform::positions::OptionType;
use crate::tt_api::sessions::*;

use crate::tt_api::mktdata::ApiQuoteToken;

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

    pub fn startup(&mut self, account_id: &str, auth_token: &str) -> ConnectAccounts {
        let connect = ConnectAccounts {
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
        let heartbeat = AccHeartbeat {
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
        if let serde_json::Result::Ok(response) = serde_json::from_str::<Response>(&response) {
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

    pub async fn startup(&mut self) -> ConnectMktData {
        ConnectMktData {
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
                let request = Auth {
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

                let request = Channel {
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

    pub fn subscribe(
        &mut self,
        symbol: Option<&str>,
        event_type: &[&str],
        option_type: OptionType,
    ) -> anyhow::Result<()> {
        fn get_channel_number(option_type: OptionType) -> u64 {
            match option_type {
                OptionType::EquityOption | OptionType::FutureOption => 1_u64,
                _ => 1_u64,
            }
        }
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
        let subscription = FeedSubscription {
            msg: Header {
                msg_type: "FEED_SUBSCRIPTION".to_string(),
                channel: get_channel_number(option_type),
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
        debug!("response {}", response);
        if let serde_json::Result::Ok(payload) = serde_json::from_str::<FeedData>(&response) {
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
                        //     accept_event_fields: Some(AcceptEventFields {
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
            // } else if let Ok(response) = serde_json::from_str::<Channel>(&response) {
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
