use anyhow::bail;
use anyhow::Result;
use crossbeam_channel::unbounded;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;

use futures::stream::Fuse;
use futures::stream::FusedStream;
use futures::stream::Map;
use futures::stream::SplitSink;
use futures::stream::SplitStream;
use futures::Future;
use futures::FutureExt as _;
use futures::Sink;
use futures::StreamExt as _;
use futures_channel::mpsc;
use serde_json::Value;
use sqlx::types::Json;
use tokio::net::TcpStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::handshake::client::Request;
use tokio_tungstenite::tungstenite::handshake::client::Response;
use tokio_tungstenite::tungstenite::http::response;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_util::sync::CancellationToken;
use tracing::info;
use url::Url;
use websocket_util::subscribe;
use websocket_util::subscribe::Classification;
use websocket_util::subscribe::Classification::ControlMessage;
use websocket_util::subscribe::Message;
use websocket_util::subscribe::MessageStream;
use websocket_util::subscribe::Subscription;
use websocket_util::wrap::Builder;
use websocket_util::wrap::Wrapper;

// /// A "dummy" message type used for testing.
// #[derive(Debug)]
// enum SocketMsg<T> {
//     /// The actual user visible message.
//     Value(T),
//     /// A "control" message.
//     Close(u8),
// }

// impl<T> Message for SocketMsg<T> {
//     type UserMessage = T;
//     type ControlMessage = u8;

//     fn classify(self) -> Classification<Self::UserMessage, Self::ControlMessage> {
//         match self {
//             SocketMsg::Value(x) => Classification::UserMessage(x),
//             SocketMsg::Close(x) => Classification::ControlMessage(x),
//         }
//     }

//     #[inline]
//     fn is_error(_user_message: &Self::UserMessage) -> bool {
//         // In this implementation there are no errors.
//         false
//     }
// }
enum SocketMsg {
    Action,
    Data(String),
}

#[derive(Debug)]
pub struct WebSocketClient {
    receiver: Receiver<SocketMsg>,
    sender: Sender<SocketMsg>,
}

impl WebSocketClient {
    pub async fn new(url_str: &str, shutdown_signal: CancellationToken) -> anyhow::Result<Self> {
        // WebSocket server URL
        let url = Url::parse(url_str)?;
        let (sender, receiver) = unbounded::<SocketMsg>();
        let receiver = match Self::subscribe_to_web_stream(url, receiver, shutdown_signal).await {
            Ok(val) => val,
            Err(err) => bail!(
                "Failed to start webstream for url: {} error: {}",
                url_str,
                err,
            ),
        };
        Ok(Self { receiver, sender })
    }

    pub fn subscribe_to_events(&self) -> Receiver<SocketMsg> {
        self.receiver.clone()
    }

    pub async fn send_message(&mut self, payload: String) -> anyhow::Result<()> {
        let payload = SocketMsg::Data(payload);
        match self.sender.send(payload) {
            Err(err) => anyhow::bail!("Error sending on crossbeam channel, error: {}", err),
            _ => Ok(()),
        }
    }

    async fn connect(url: &Url) -> Result<Wrapper<WebSocketStream<MaybeTlsStream<TcpStream>>>> {
        async fn inner(url: &Url) -> anyhow::Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
            let (stream, _response) = connect_async(url).await?;
            anyhow::Result::Ok(stream)
        }

        let stream = inner(url)
            .await
            .map(|channel| Wrapper::builder().build(channel))?;

        anyhow::Ok(stream)
    }

    async fn subscribe_to_web_stream(
        url: Url,
        receiver: Receiver<SocketMsg>,
        shutdown_signal: CancellationToken,
    ) -> anyhow::Result<Receiver<SocketMsg>> {
        let mut stream = Self::connect(&url).await?;
        let (send, recv) = stream.into();
        let (mut stream, subscription) = subscribe::subscribe(send, recv);
        let mut stream = stream.into().fuse();
        let connect = subscription.into().read().boxed();
        let message = subscribe::drive(connect, &mut stream).await?;

        match message {
            Some(Ok(val)) => info!("Failed to validate, {}", val),
            Some(Err(err)) => anyhow::bail!("failed to read connected message, error: {:?}", err),

            None => {
                return Err(anyhow::anyhow!(
                    "stream was closed before connected message was received",
                ))
            }
        }

        let (sender, receiver) = unbounded::<SocketMsg>();
        // tokio::spawn(async move {
        //     loop {
        //         tokio::select! {
        //             event = receiver.recv() => {
        //                 match event {
        //                     std::result::Result::Ok(SocketMsg { action, data }) => {
        //                         let subscribe = match action {
        //                             SubscriptType::Subscribe => {
        //                                 debug!("Received subscribed for symbol list: {:?}", data);
        //                                 subscription.send(&data).boxed().fuse()
        //                             },
        //                         };
        //                         if let Err(err) = stream::drive(subscribe, &mut stream).await.unwrap().unwrap() {
        //                                 error!("Subscribe error in the stream drive: {err:?}");
        //                                 shutdown_signal.cancel();
        //                                 break
        //                         };
        //                     }
        //                     Err(RecvError::Lagged(err)) => warn!("Publisher channel skipping a number of messages: {}", err),
        //                     Err(RecvError::Closed) => {
        //                         error!("Publisher channel closed");
        //                         shutdown_signal.cancel();
        //                         break
        //                     }
        //                 }
        //             },
        //             payload = stream.next() => {
        //                 let shutdown = shutdown_signal.clone();
        //                 tokio::spawn(async move {
        //                     if let Some(data) = payload {
        //                         let data = match data {
        //                             std::result::Result::Ok(val) => val,
        //                             Err(err) => {
        //                                 shutdown.cancel();
        //                                 return warn!("Failed to parse data, error={}", err);
        //                             }
        //                         };
        //                         let data = match data {
        //                             std::result::Result::Ok(val) => val,
        //                             Err(err) => {
        //                                 shutdown.cancel();
        //                                 return warn!("Failed to parse data, error={}", err);
        //                             }
        //                         };
        //                         let event = match data {
        //                             stream::Data::Trade(data) => Event::Trade(data),
        //                             stream::Data::Quote(data) => Event::Quote(data),
        //                             stream::Data::Bar(data) => Event::Bar(data),
        //                             _ => return,
        //                         };
        //                         let mut retries = 5;
        //                         while let Err(broadcast::error::SendError(data)) = sender.send(event.clone()) {
        //                             error!("{data:?}");
        //                             match retries {
        //                                 0 => {
        //                                     error!("Max retries reached, closing app");
        //                                     shutdown.cancel();
        //                                     break
        //                                 },
        //                                 _ => retries -= 1
        //                             }
        //                         }
        //                     };
        //                 });
        //             }
        //             _ = shutdown_signal.cancelled() => {
        //                 break
        //             }
        //         }
        //     }
        // });
        Ok(receiver)
    }
}
