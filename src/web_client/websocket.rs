use anyhow::bail;
use anyhow::Ok;
use tokio_tungstenite::Connector;
use tokio_tungstenite::MaybeTlsStream;
use websocket_util::subscribe::Classification;
use websocket_util::subscribe::Message;
// use crossbeam_channel::unbounded;
// use crossbeam_channel::Receiver;
// use crossbeam_channel::Sender;
use native_tls;
use native_tls::Protocol;
use native_tls::TlsConnector as NativeTlsConnector;
use serde::Deserialize;
use serde::Serialize;
use serde_json::to_string as to_json;
use serde_json::Error as JsonError;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::Sender;
use tokio_tungstenite::WebSocketStream;
use tracing::error;
use tracing::info;
use websocket_util::tungstenite::Error as WebSocketError;
use websocket_util::wrap;
use websocket_util::wrap::Wrapper;

use url::Url;

use core::result::Result as CoreResult;

/// A "dummy" message type used for testing.
#[derive(Debug)]
pub enum TtMessage {
    Application(String),
    Session(String),
}

impl Message for TtMessage {
    type UserMessage = String;
    type ControlMessage = String;

    fn classify(self) -> Classification<Self::UserMessage, Self::ControlMessage> {
        match &self {
            TtMessage::Application(x) => Classification::UserMessage(x.to_string()),
            TtMessage::Session(x) => Classification::ControlMessage(x.to_string()),
        }
    }

    fn is_error(user_message: &Self::UserMessage) -> bool {
        false
    }
}

#[derive(Debug)]
pub struct WebSocketClient {
    // receiver: Receiver<SocketMsg>,
    // sender: Sender<SocketMsg>,
    client: Wrapper<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    receiver: Receiver<String>,
    sender: Sender<String>,
}

fn message_parser(
    result: Result<wrap::Message, WebSocketError>,
) -> Result<Result<TtMessage, JsonError>, WebSocketError> {
    result.map(|message| match message {
        wrap::Message::Text(string) => json_from_str::<TtMessage>(&string),
        wrap::Message::Binary(data) => json_from_slice::<Vec<DataMessage<B, Q, T>>>(&data),
    })
}

impl WebSocketClient {
    pub async fn new(url: &str) -> anyhow::Result<Self> {
        // WebSocket server URL
        info!("Creating websocket with target host: {}", url);

        let (sender, receiver) = broadcast::channel(300);

        let tls_connector = NativeTlsConnector::builder()
            .min_protocol_version(Some(Protocol::Tlsv12))
            .build()?;

        // let tls_connector = TlsConnector::from(tls_connector);
        let stream = TcpStream::connect(url).await?;
        let (stream, _) = tokio_tungstenite::client_async_tls_with_config(
            url,
            stream,
            None,
            Some(Connector::NativeTls(tls_connector)),
        )
        .await?;

        let client = Ok(Wrapper::builder()
            .set_ping_interval(Some(Duration::from_secs(15)))
            .build(stream))
        .map(|m| message_parser(m));

        Ok(Self {
            client,
            receiver,
            sender,
        })
    }

    pub fn subscribe_to_events(&self) -> Receiver<String> {
        // pub fn subscribe_to_events(&self) -> Receiver<SocketMsg> {
        // self.receiver.clone()
        self.sender.subscribe()
    }

    pub async fn send_message<Payload>(&mut self, payload: Payload) -> anyhow::Result<()>
    where
        Payload: Serialize + for<'a> Deserialize<'a>,
    {
        let output = format!("to websocket sending payload: {}", to_json(&payload)?);
        info!("output: {}", output);
        match self.client.text(to_json(&payload)?) {
            Err(err) => anyhow::bail!("Error sending payload to websocket stream, error: {}", err),
            _ => Ok(()),
        }
    }

    // pub async fn cancel_stream(&self) {
    //     let _ = self.client.close(Some(ezsockets::CloseFrame {
    //         code: ezsockets::CloseCode::Normal,
    //         reason: String::from("Closing websocket stream"),
    //     }));
    // }

    // async fn subscribe_to_web_stream(
    //     url: Url,
    //     sender: Sender<String>,
    //     // sender: Sender<SocketMsg>,
    // ) -> Result<Client<ClientMsgHandler>> {
    //     let config = ClientConfig::new(url);
    //     let protocal_version = vec![&TLS12];

    //     let mut root_store = rustls::RootCertStore::empty();
    //     root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    //     let tls_config = Config::builder()
    //         .with_root_certificates(root_store)
    //         .with_no_client_auth();

    //     let (client, future) =
    //         ezsockets::connect(move |_client| ClientMsgHandler { sender }, config).await;

    //     let wrapped_client = tls_config.connect_async(Arc::new(client.0)).await.unwrap();

    //     ezsockets::ClientConfig::default();
    //     tokio::spawn(async move {
    //         match future.await {
    //             CoreResult::Ok(val) => info!("Future exited gracefully, response: {:?}", val),
    //             Err(err) => error!("Error thrown from the future, error: {:?}", err),
    //         }
    //     });
    //     Ok(client)
    // }
}
