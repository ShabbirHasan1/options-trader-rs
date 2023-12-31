use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use serde_json::from_slice as json_from_slice;
use serde_json::from_str as json_from_str;
use serde_json::to_string as to_json;
use serde_json::Error as JsonError;
use sqlx::postgres::PgArguments;
use sqlx::postgres::PgRow;
use sqlx::query::Query;
use sqlx::FromRow;
use sqlx::Postgres;
use sqlx::Row;
use std::fmt::format;
use tracing::info;

use ezsockets::Server as EzServer;

mod http_client;
mod tt_api;
mod websocket;

use super::db_client::DBClient;
use http_client::HttpClient;
use websocket::WebSocketClient;

#[cfg(test)]
const BASE_URL_UAT: &str = "api.cert.tastyworks.com";
#[cfg(test)]
const WS_URL_UAT: &str = "streamer.cert.tastyworks.com";

#[cfg(not(test))]
const BASE_URL_UAT: &str = "api.cert.tastyworks.com";
#[cfg(not(test))]
const WS_URL_UAT: &str = "streamer.cert.tastyworks.com";

#[derive(Debug, Serialize, Deserialize)]
struct RefreshToken {
    data: Data,
    context: String,
}

#[derive(FromRow, Debug, Serialize, Deserialize)]
struct Data {
    user: User,
    session_token: String,
    remember_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct User {
    email: String,
    username: String,
    external_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AuthResponse {
    rememberme: String,
    session_id: String,
}

pub struct WebClient {
    http_client: HttpClient,
    websocket: WebSocketClient,
    db: DBClient,
}

impl WebClient {
    pub async fn new(base_url: &str, ws_url: &str, db: DBClient) -> Result<Self> {
        Ok(WebClient {
            http_client: HttpClient::new(base_url),
            websocket: WebSocketClient::new(&WS_URL_UAT).await?,
            db,
        })
    }

    async fn refresh_session_token(&self, data: RefreshToken) -> Result<AuthResponse> {
        let refresh_token = self
            .http_client
            .post::<RefreshToken, AuthResponse>("sessions", data)
            .await?;
        info!("Refresh token success, token: {:?}", refresh_token);
        Ok(refresh_token)
    }

    async fn subscribe_to_account_updates() {}

    async fn subscribe_to_mktdata_updates() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    use mockito::Matcher;
    use mockito::Server;

    use crate::settings::Settings;
    use crate::utils::ws_server::WsServer;

    fn auth_token_response() -> RefreshToken {
        RefreshToken {
            data: Data {
                user: User {
                    email: "chrislozza@gmail.com".to_string(),
                    username: "chrislozza".to_string(),
                    external_id: "".to_string(),
                },
                session_token: "09283453092384023582048592".to_string(),
                remember_token: "09384345762294387902384509823".to_string(),
            },
            context: "application/json".to_string(),
        }
    }

    async fn get_ws_server<CustomServer>() {
        let (server, _) = EzServer::new(|_server| WsServer {});
        ezsockets::tungstenite::run(server, "127.0.0.1:8080")
            .await
            .unwrap();
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct AuthResponse {
        rememberme: String,
        session_id: String,
    }

    #[tokio::test]
    async fn test_refresh_session_token() {
        let mut server = Server::new();
        let url = server.url();
        let root = auth_token_response();

        let client = WebClient::new(&url, &WS_URL_UAT).await;
        assert!(client.is_ok());
        let client = client.unwrap();

        let response = AuthResponse {
            rememberme: "905802385048509348503985".to_string(),
            session_id: "098573495709283498724359872".to_string(),
        };

        // Create a mock
        let mock = server
            .mock("POST", "/sessions")
            .match_header("content-type", "application/json")
            .match_body(Matcher::JsonString(to_json(&root).unwrap()))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(to_json(&response).unwrap())
            .create();

        let client = client.refresh_session_token(root).await;
        assert!(client.is_ok());
        mock.assert();
    }

    #[tokio::test]
    async fn test_connect_to_web_socket() {}

    #[ignore = "live-test"]
    #[tokio::test]
    async fn test_live_download_account_details() {
        // Specify the URL you want to use for testing
        let client = WebClient::new(&BASE_URL_UAT, &WS_URL_UAT).await;

        assert!(client.is_ok());

        let root = auth_token_response();

        let client = client.unwrap();
        assert!(client.refresh_session_token(root).await.is_ok());
    }
}
