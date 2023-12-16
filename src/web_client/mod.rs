use anyhow::Result;

mod http_client;
mod websocket;

use http_client::HttpClient;
use websocket::WebSocketClient;

pub struct WebClient {
    http_client: HttpClient,
    websocket: WebSocketClient,
}

//http requests sandbox
//api.cert.tastyworks.com

//websocket
const UAT_STREAM: String = format!("streamer.cert.tastyworks.com");

impl WebClient {
    pub async fn new() -> Result<Self, Error> {
        Ok(WebClient {
            http_client: HttpClient::new().await?,
            websocket: WebSocketClient::new(&UAT_STREAM).await?,
        })
    }
}
