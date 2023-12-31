use serde::de::value::UsizeDeserializer;
use serde::de::DeserializeOwned;
use serde::de::Deserializer;
use serde::ser::SerializeSeq as _;
use serde::ser::Serializer;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Error as JsonError;
use tracing::error;

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Message {
    uid: String,
    #[serde(rename = "type")]
    message_type: String,
    channel: i64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ErrorMessage {
    #[serde(flatten)]
    message: Message,
    error: ErrorDetails,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ErrorDetails {
    #[serde(rename = "type")]
    error_type: String,
    message: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
struct AuthMessage {
    #[serde(flatten)]
    base: Message,
    #[serde(rename = "type")]
    message_type: String, // Should always be "AUTH"
    token: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ChannelRequestMessage {
    #[serde(flatten)]
    base: Message,
    #[serde(rename = "type")]
    message_type: String, // Should always be "CHANNEL_OPEN"
    service: Service,
    parameters: ServiceParameters,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Service {
    Feed,
}

#[derive(Serialize, Deserialize, Debug)]
struct ServiceParameters {
    contract: FeedContract,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum FeedContract {
    History,
    Ticker,
    Stream,
    Auto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StreamApiError {
    #[serde(rename = "code")]
    pub code: u64,
    #[serde(rename = "msg")]
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum WsRequest {
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "error")]
    Error(StreamApiError),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum WsResponse {
    /// A variant representing aggregate data for a given symbol.
    #[serde(rename = "a")]
    Payload(AuthMessage),
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "error")]
    Error(StreamApiError),
}
