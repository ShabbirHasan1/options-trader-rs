use anyhow::bail;
use anyhow::Ok;
use anyhow::Result;
use async_trait::async_trait;
// use crossbeam_channel::unbounded;
// use crossbeam_channel::Receiver;
// use crossbeam_channel::Sender;
use ezsockets::Client;
use ezsockets::ClientConfig;
use ezsockets::ClientConnectorTokio;
use ezsockets::ClientExt;
use rustls::version::TLS12;
use rustls::ClientConfig as Config;
use rustls::RootCertStore;
use rustls::SupportedProtocolVersion;
use serde::Deserialize;
use serde::Serialize;
use serde_json::to_string as to_json;
use std::sync::Arc;
use tokio::sync::broadcast;

use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::Sender;
use tracing::error;
use tracing::info;

use url::Url;

use core::result::Result as CoreResult;

// #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// pub enum SocketMsg {
//     Response(String),
// }

#[derive(Debug)]
struct ClientMsgHandler {
    // sender: Sender<SocketMsg>,
    sender: Sender<String>,
}

#[async_trait]
impl ClientExt for ClientMsgHandler {
    type Call = ();

    async fn on_text(&mut self, text: String) -> CoreResult<(), ezsockets::Error> {
        info!("received message: {text}");

        match self.sender.send(text) {
            Err(err) => {
                panic!("Something went wrong, error: {}", err)
            }
            _ => CoreResult::Ok(()),
        }
        // self.sender.send(SocketMsg::Response(text));
    }

    async fn on_binary(&mut self, bytes: Vec<u8>) -> CoreResult<(), ezsockets::Error> {
        info!("received bytes: {bytes:?}");
        CoreResult::Ok(())
    }

    async fn on_call(&mut self, call: Self::Call) -> CoreResult<(), ezsockets::Error> {
        let () = call;
        CoreResult::Ok(())
    }
}

#[derive(Debug)]
pub struct WebSocketClient {
    client: Client<ClientMsgHandler>,
    // receiver: Receiver<SocketMsg>,
    // sender: Sender<SocketMsg>,
    receiver: Receiver<String>,
    sender: Sender<String>,
}

impl WebSocketClient {
    pub async fn new(url_str: &str) -> anyhow::Result<Self> {
        // WebSocket server URL
        info!("Creating websocket with target host: {}", url_str);

        let url = Url::parse(url_str)?;
        let (sender, receiver) = broadcast::channel(300);
        // let (sender, receiver) = unbounded::<SocketMsg>();
        let client = match Self::subscribe_to_web_stream(url, sender.clone()).await {
            CoreResult::Ok(val) => val,
            Err(err) => bail!(
                "Failed to start webstream for url: {} error: {}",
                url_str,
                err,
            ),
        };
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

    pub async fn cancel_stream(&self) {
        let _ = self.client.close(Some(ezsockets::CloseFrame {
            code: ezsockets::CloseCode::Normal,
            reason: String::from("Closing websocket stream"),
        }));
    }

    async fn subscribe_to_web_stream(
        url: Url,
        sender: Sender<String>,
        // sender: Sender<SocketMsg>,
    ) -> Result<Client<ClientMsgHandler>> {
        let config = ClientConfig::new(url);
        let protocal_version = vec![&TLS12];

        let mut root_store = rustls::RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        let tls_config = Config::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        let (client, future) =
            ezsockets::connect(move |_client| ClientMsgHandler { sender }, config).await;

        let wrapped_client = tls_config. .connect_async(Arc::new(client.0)).await.unwrap();

        ezsockets::ClientConfig::default();
        tokio::spawn(async move {
            match future.await {
                CoreResult::Ok(val) => info!("Future exited gracefully, response: {:?}", val),
                Err(err) => error!("Error thrown from the future, error: {:?}", err),
            }
        });
        Ok(client)
    }
}
