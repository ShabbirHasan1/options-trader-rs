use std::sync::Arc;
use std::sync::RwLock;
use tokio::net::TcpStream;
use tokio_native_tls::TlsConnector;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::connect_async;
use tracing::info;
use url::Url;

type Stream = WebSocketStream<TlsConnector>;

#[derive(Debug)]
pub struct WebSocketClient {
    write: Stream,
    read: Stream,
}

impl WebSocketClient {
    pub async fn new(url_str: &str) -> Result<Self, Box<dyn std::error::Error>> {
        // WebSocket server URL
        let url = Url::parse(url_str).expect("Invalid URL");
        let stream = TcpStream::connect(url.host_str().unwrap())
            .await
            .expect("Failed to connect");
        let stream = Stream::::connect(url.host_str().unwrap(), stream)
            .connect()
            .await
            .expect("TLS handshake failed");
        let (write, read) = connect_async(url.clone())
            .await
            .expect("Failed to connect to WebSocket");

        Ok(Self { write, read })
    }

    pub async fn send_message(&mut self, message: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.write
            .send(Message::text(message))
            .await
            .expect("Failed to send message");
        Ok(())
    }
    pub async fn receive_messages(self) -> Result<(), Box<dyn std::error::Error>> {
        let read_stream = Arc::new(RwLock::new(self.read));

        // Spawn a task to handle WebSocket messages
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(Ok(message)) = read_stream.read().next() => {
                        match message {
                            Message::Text(text) => {
                                // Assuming your JSON messages are strings
                                info!("Received JSON message: {}", text);

                                // Deserialize JSON
                                if let Ok(json_value) = serde_json::from_str::<Value>(&text) {
                                    info!("Parsed JSON: {:#?}", json_value);
                                } else {
                                    info!("Failed to parse JSON");
                                }
                            }
                            Message::Binary(_) => {
                                info!("Received binary message");
                            }
                            Message::Ping(_) => {
                                info!("Received ping");
                            }
                            Message::Pong(_) => {
                                info!("Received pong");
                            }
                            Message::Close(_) => {
                                info!("Received close");
                                break;
                            }
                            _ => info!("Unknown")
                        }
                    }
                    else => {
                        // Handle other branches or add additional tasks/futures here
                    }
                }
            }
        });

        Ok(())
    }
}
