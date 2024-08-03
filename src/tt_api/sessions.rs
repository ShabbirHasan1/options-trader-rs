use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;
use sqlx::FromRow;

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AccHeartbeat {
    pub action: String,
    pub auth_token: String,
}

#[derive(Clone, Default, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Response {
    pub status: String,
    pub action: String,
    pub websocket_session_id: String,
    pub value: Option<Vec<String>>,
    pub request_id: u8,
}

#[derive(FromRow, Clone, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ConnectAccounts {
    pub action: String,
    #[serde(rename = "value")]
    pub account_ids: Vec<String>,
    #[serde(rename = "auth-token")]
    pub auth_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Payload {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub data: String,
    pub timestamp: u32,
}

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
pub struct ConnectMktData {
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
