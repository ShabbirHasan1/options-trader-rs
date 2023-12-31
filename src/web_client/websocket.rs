use anyhow::bail;
use anyhow::Result;
use async_trait::async_trait;
use crossbeam_channel::unbounded;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use ezsockets::Client;
use ezsockets::ClientConfig;
use ezsockets::ClientExt;
use reqwest::Response;
use serde_json::error;
use serde_json::from_slice as json_from_slice;
use serde_json::from_str as json_from_str;
use serde_json::to_string as to_json;
use serde_json::Error as JsonError;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::info;
use tracing::warn;
use url::Url;

use core::result::Result as CoreResult;

use super::tt_api::WsResponse;

enum SocketMsg {
    Request(String),
    Response(String),
}

#[derive(Debug)]
struct ClientMsgHandler {
    sender: Sender<SocketMsg>,
}

#[async_trait]
impl ClientExt for ClientMsgHandler {
    type Call = ();

    async fn on_text(&mut self, text: String) -> CoreResult<(), ezsockets::Error> {
        info!("received message: {text}");

        self.sender.send(SocketMsg::Response(text));

        Ok(())
    }

    async fn on_binary(&mut self, bytes: Vec<u8>) -> CoreResult<(), ezsockets::Error> {
        info!("received bytes: {bytes:?}");
        unimplemented!();
    }

    async fn on_call(&mut self, call: Self::Call) -> CoreResult<(), ezsockets::Error> {
        let () = call;
        Ok(())
    }
}

#[derive(Debug)]
pub struct WebSocketClient {
    client: Client<ClientMsgHandler>,
    receiver: Receiver<SocketMsg>,
    sender: Sender<SocketMsg>,
}

impl WebSocketClient {
    pub async fn new(url_str: &str) -> anyhow::Result<Self> {
        // WebSocket server URL
        let url = Url::parse(url_str)?;
        let (sender, receiver) = unbounded::<SocketMsg>();
        let client = match Self::subscribe_to_web_stream(url, sender.clone()).await {
            Ok(val) => val,
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

    pub fn subscribe_to_events(&self) -> Receiver<SocketMsg> {
        self.receiver.clone()
    }

    pub async fn send_message(&mut self, payload: String) -> anyhow::Result<()> {
        match self.client.text(payload) {
            Err(err) => anyhow::bail!("Error sending on crossbeam channel, error: {}", err),
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
        sender: Sender<SocketMsg>,
    ) -> Result<Client<ClientMsgHandler>> {
        let config = ClientConfig::new(url);
        let (client, future) =
            ezsockets::connect(move |_client| ClientMsgHandler { sender }, config).await;

        tokio::spawn(async move {
            match future.await {
                Ok(val) => info!("Future exited gracefully, response: {:?}", val),
                Err(err) => error!("Error thrown from the future, error: {:?}", err),
            }
        });
        Ok(client)
    }
}
