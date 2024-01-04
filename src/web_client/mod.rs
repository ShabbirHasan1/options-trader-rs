use anyhow::bail;
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
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::error;
use tracing::info;

use ezsockets::Server as EzServer;

mod http_client;
mod tt_api;
mod websocket;

use crate::db_client::SqlQueryBuilder;

use super::db_client::DBClient;
use super::settings::Settings;
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
    data: AuthResponse,
    context: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EndPoint {
    #[default]
    Sandbox,
    Live,
}

impl FromRow<'_, PgRow> for EndPoint {
    fn from_row(row: &PgRow) -> sqlx::Result<Self> {
        sqlx::Result::Ok(match row.try_get("endpoint")? {
            1 => EndPoint::Sandbox,
            2 => EndPoint::Live,
            _ => panic!("Unknown endpoint"),
        })
    }
}

impl Into<i32> for EndPoint {
    fn into(self) -> i32 {
        match self {
            EndPoint::Sandbox => 1,
            EndPoint::Live => 2,
        }
    }
}

#[derive(FromRow, Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
struct AuthCreds {
    #[serde(rename = "login")]
    username: String,
    #[serde(rename = "remember-token")]
    remember: String,
    #[sqlx(flatten)]
    #[serde(skip)]
    endpoint: EndPoint,
    #[serde(rename = "remember-me")]
    #[sqlx(default)]
    remember_me: bool,
}

impl Default for AuthCreds {
    fn default() -> Self {
        Self {
            username: String::default(),
            remember: String::default(),
            endpoint: EndPoint::default(),
            remember_me: true, // Set the default value for remember_me here
        }
    }
}
#[derive(FromRow, Clone, Default, Debug, Serialize, Deserialize)]
struct AuthResponse {
    #[sqlx(flatten)]
    user: User,
    #[serde(rename = "session-token")]
    session: String,
    #[serde(rename = "remember-token")]
    remember: String,
}

#[derive(FromRow, Clone, Default, Debug, Serialize, Deserialize)]
struct User {
    email: String,
    username: String,
    #[serde(rename = "external-id")]
    external_id: String,
}

pub struct WebClient {
    session: Option<String>,
    http_client: HttpClient,
    websocket: WebSocketClient,
}

impl WebClient {
    pub async fn new(base_url: &str, ws_url: &str) -> Result<Self> {
        Ok(WebClient {
            session: None,
            http_client: HttpClient::new(&format!("https://{}", base_url)),
            websocket: WebSocketClient::new(&&format!("ws://{}", ws_url)).await?,
        })
    }

    pub async fn startup(mut self, settings: Settings, db: &DBClient) -> Result<Self> {
        let creds = Self::fetch_auth_from_db(&settings.username, settings.endpoint, db).await?;
        assert!(creds.len() == 1);
        let data = &creds[0];

        let updates = match Self::refresh_session_token(&self.http_client, data.clone()).await {
            Ok(val) => {
                Self::update_auth_from_db(&val.data.remember, settings.endpoint, db).await?;
                val
            }
            Err(err) => bail!("Failed to update refresh token, error: {}", err),
        };

        self.session = Some(updates.data.session);
        Ok(self)
    }

    async fn fetch_auth_from_db(
        username: &str,
        endpoint: EndPoint,
        db: &DBClient,
    ) -> Result<Vec<AuthCreds>> {
        let columns = vec!["username", "endpoint"];

        let stmt = SqlQueryBuilder::prepare_fetch_statement("tasty_auth", &columns);
        match sqlx::query_as::<_, AuthCreds>(&stmt)
            .bind(username.to_string())
            .bind::<i32>(endpoint.into())
            .fetch_all(&db.pool)
            .await
        {
            sqlx::Result::Ok(val) => Ok(val),
            Err(err) => bail!(
                "Failed to fetch transactions from db, err={}, closing app",
                err
            ),
        }
    }

    async fn update_auth_from_db(remember: &str, endpoint: EndPoint, db: &DBClient) -> Result<()> {
        let stmt =
            SqlQueryBuilder::prepare_update_statement("tasty_auth", &vec!["remember", "endpoint"]);

        match sqlx::query(&stmt)
            .bind(remember)
            .bind::<i32>(endpoint.into())
            .execute(&db.pool)
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => bail!("Failed to publish to db, error={}", err),
        }
    }

    async fn refresh_session_token(
        http_client: &HttpClient,
        mut data: AuthCreds,
    ) -> Result<RefreshToken> {
        data.remember_me = true;
        let refresh_token = http_client
            .post::<AuthCreds, RefreshToken>("sessions", data, None)
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
    use tracing_subscriber::fmt::format;

    use crate::settings::Config;
    use crate::settings::DatabaseConfig;
    use crate::settings::Settings;
    use crate::utils::mock_db::MockDb;
    use crate::utils::ws_server::WsServer;
    use std::env;
    use std::{thread, time};

    // fn auth_token_response() -> AuthCreds {
    //     AuthCreds {
    //         username: "test_account".to_string(),
    //         remember: "09384345762294387902384509823".to_string(),
    //         endpoint: EndPoint::Sandbox,
    //     }
    // }

    // async fn get_ws_server<CustomServer>() {
    //     let (server, _) = EzServer::create(|_server| WsServer {});
    //     ezsockets::tungstenite::run(server, "127.0.0.1:8080")
    //         .await
    //         .unwrap();
    // }

    #[derive(Debug, Serialize, Deserialize)]
    struct AuthResponse {
        rememberme: String,
        session_id: String,
    }

    // #[tokio::test]
    // async fn test_refresh_session_token() {
    //     let mut server = Server::new();
    //     let url = server.url();
    //     let auth_creds = &auth_token_response();
    //     let client = WebClient::new(&url, &WS_URL_UAT).await;
    //     assert!(client.is_ok());
    //     let client = client.unwrap();

    //     let response = AuthResponse {
    //         rememberme: "905802385048509348503985".to_string(),
    //         session_id: "098573495709283498724359872".to_string(),
    //     };

    //     // Create a mock
    //     let mock = server
    //         .mock("POST", "/sessions")
    //         .match_header("content-type", "application/json")
    //         .match_body(Matcher::JsonString(to_json(&auth_creds).unwrap()))
    //         .with_status(201)
    //         .with_header("content-type", "application/json")
    //         .with_body(to_json(&response).unwrap())
    //         .create();

    //     let client = WebClient::refresh_session_token(&client.http_client, auth_creds).await;
    //     assert!(client.is_ok());
    //     mock.assert();
    // }

    // #[tokio::test]
    // async fn test_connect_to_web_socket() {}

    // #[ignore = "live-test"]
    #[tokio::test]
    async fn test_live_download_account_details() {
        // Specify the URL you want to use for testing
        let config = env::var("OPTIONS_CFG")
            .expect("Failed to get the cfg file from the environment variable.");
        let settings = Config::read_config_file(&config).expect("Failed to parse config file");
        let db = DBClient::new(&settings).await.unwrap();

        let client = WebClient::new(&BASE_URL_UAT, &WS_URL_UAT).await;
        assert!(client.is_ok());

        let mut stored_auths: (AuthCreds, AuthCreds) = Default::default();
        for index in 0..2 {
            let auth_creds =
                WebClient::fetch_auth_from_db(&settings.username, settings.endpoint, &db).await;
            assert!(auth_creds.is_ok());

            let stored_data = match index {
                0 => {
                    stored_auths.0 = auth_creds.unwrap()[0].clone();
                    stored_auths.clone().0
                }

                1 => {
                    stored_auths.1 = auth_creds.unwrap()[0].clone();
                    stored_auths.clone().1
                }
                _ => panic!("Index out of range"),
            };
            let client = client.as_ref().unwrap();
            let refreshed =
                match WebClient::refresh_session_token(&client.http_client, stored_data).await {
                    Ok(val) => Ok(val),
                    Err(err) => {
                        let error = format!("{}", err);
                        Err(error)
                    }
                };
            assert!(refreshed.is_ok());

            let result = match WebClient::update_auth_from_db(
                &refreshed.unwrap().data.remember,
                settings.endpoint,
                &db,
            )
            .await
            {
                Ok(val) => Ok(val),
                Err(err) => {
                    let error = format!("{}", err);
                    Err(error)
                }
            };

            assert!(result.is_ok());

            let second = time::Duration::from_secs(1);
            thread::sleep(second);
        }

        let first = stored_auths.clone().0;
        let second = stored_auths.clone().1;
        assert!(first.ne(&second));
        // let updated_data = refreshed.unwrap();
        // assert!(updated_data.session.ne(&stored_data.session));
        // assert!(updated_data.remember.ne(&stored_data.remember));
    }
}
