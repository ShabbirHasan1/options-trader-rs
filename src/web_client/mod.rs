use anyhow::bail;
use anyhow::Ok;
use anyhow::Result;
use chrono::DateTime;
use chrono::Utc;
use core::result::Result as CoreResult;
use serde::Deserialize;
use serde::Serialize;
use sqlx::postgres::PgRow;
use sqlx::FromRow;
use sqlx::Row;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::Sender;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

mod http_client;
mod sessions;
mod tt_api;
mod websocket;

use crate::db_client::SqlQueryBuilder;

use super::db_client::DBClient;
use super::settings::Settings;
use http_client::HttpClient;
use sessions::AccountSession;
use sessions::MktdataSession;
use websocket::WebSocketClient;
// use websocket::SocketMsg;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApiQuoteToken {
    token: String,
    #[serde(rename = "streamer-url")]
    streamer_url: String,
    #[serde(rename = "websocket-url")]
    websocket_url: String,
    #[serde(rename = "dxlink-url")]
    dxlink_url: String,
    level: String,
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

impl From<EndPoint> for i32 {
    fn from(val: EndPoint) -> Self {
        match val {
            EndPoint::Sandbox => 1,
            EndPoint::Live => 2,
        }
    }
}

#[derive(FromRow, Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
struct AuthCreds {
    #[serde(rename = "login")]
    username: String,
    #[serde(skip)]
    account: String,
    #[serde(skip)]
    session: String,
    #[serde(rename = "remember-token")]
    remember: String,
    #[sqlx(flatten)]
    #[serde(skip)]
    endpoint: EndPoint,
    #[serde(rename = "remember-me")]
    #[sqlx(default)]
    remember_me: bool,
    #[serde(skip)]
    last: DateTime<Utc>,
}

impl Default for AuthCreds {
    fn default() -> Self {
        Self {
            username: String::default(),
            account: String::default(),
            session: String::default(),
            remember: String::default(),
            endpoint: EndPoint::default(),
            remember_me: true,
            last: DateTime::default(),
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
struct WsConnect {
    action: String,
    #[serde(rename = "value")]
    account_ids: Vec<String>,
    #[serde(rename = "auth-token")]
    pub auth_token: String,
}

#[derive(FromRow, Clone, Default, Debug, Serialize, Deserialize)]
struct User {
    email: String,
    username: String,
    #[serde(rename = "external-id")]
    external_id: String,
}

#[derive(Clone, Debug)]
pub struct WebClient {
    session: String,
    account: String,
    http_client: HttpClient,
    account_updates: Option<WebSocketClient<AccountSession>>,
    mktdata: Option<WebSocketClient<MktdataSession>>,
    to_app: Sender<String>,
    cancel_token: CancellationToken,
}

impl WebClient {
    pub async fn new(
        base_url: &str,
        ws_url: &str,
        cancel_token: CancellationToken,
    ) -> Result<Self> {
        let (to_app, _) = broadcast::channel::<String>(100);

        Ok(WebClient {
            session: String::default(),
            account: String::default(),
            http_client: HttpClient::new(&format!("https://{}", base_url)),
            account_updates: None,
            mktdata: None,
            to_app,
            cancel_token,
        })
    }

    pub async fn startup(
        &mut self,
        account_session_url: &str,
        settings: Settings,
        db: &DBClient,
    ) -> Result<()> {
        let creds = Self::fetch_auth_from_db(&settings.username, settings.endpoint, db).await?;
        assert!(creds.len() == 1);
        let data = &creds[0];

        let updates = match Self::refresh_session_token(&self.http_client, data.clone()).await {
            CoreResult::Ok(val) => {
                Self::update_auth_from_db(
                    &val.data.session,
                    &val.data.remember,
                    settings.endpoint,
                    db,
                )
                .await?;
                val
            }
            Err(err) => bail!("Failed to update refresh token, error: {}", err),
        };

        let (to_ws, _) = broadcast::channel::<String>(100);

        let account_session =
            AccountSession::new(&format!("wss://{}", account_session_url), to_ws.clone());

        self.session = updates.data.session;
        self.account = updates.data.user.external_id;

        let cancel_token = self.cancel_token.clone();

        let api_quote_token = self.get_api_quote_token(&self.http_client, account_session.read().unwrap().get_auth_token()).await?;
        self.account_updates = Some(
            WebSocketClient::<AccountSession>::new(
                account_session,
                self.to_app.clone(),
                cancel_token.clone(),
            )
            .await?,
        );
        self.subscribe_to_account_updates().await?;
        let mktdata_session = MktdataSession::new(api_quote_token, to_ws);
        self.mktdata = Some(
            WebSocketClient::<MktdataSession>::new(
                mktdata_session,
                self.to_app.clone(),
                cancel_token,
            )
            .await?,
        );
        self.subscribe_to_mktdata().await
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

    async fn update_auth_from_db(
        session: &str,
        remember: &str,
        endpoint: EndPoint,

        db: &DBClient,
    ) -> Result<()> {
        let stmt = SqlQueryBuilder::prepare_update_statement(
            "tasty_auth",
            &vec!["session", "remember", "endpoint"],
        );

        debug!(
            "Writing remember token {} to db for statement {}",
            remember, stmt
        );
        match sqlx::query(&stmt)
            .bind(session)
            .bind(remember)
            .bind::<i32>(endpoint.into())
            .execute(&db.pool)
            .await
        {
            CoreResult::Ok(_) => Ok(()),
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

    async fn get_api_quote_token(&self,
        http_client: &HttpClient,
        auth_token: &str,
    ) -> Result<ApiQuoteToken> {
        http_client
            .get::<ApiQuoteToken>("api-quote-tokens", Some(auth_token))
            .await
    }

    pub fn subscribe_to_events(&self) -> Receiver<String> {
        self.to_app.subscribe()
    }

    async fn subscribe_to_account_updates(&mut self) -> Result<()> {
        if let Err(err) = self.account_updates.expect("Account updates not initialised").subscribe_to_events().await {
            bail!("Failed to subscribe to websocket stream, error: {}", err)
        }
        let ws_auth = WsConnect {
            action: "connect".to_string(),
            account_ids: vec!["5WY06911".to_string()],
            auth_token: self.session.to_string(),
        };
        self.account_updates.startup(ws_auth).await
    }

    async fn subscribe_to_mktdata(&mut self) -> Result<()> {
        match self.mktdata.expect("Account updates not initialised").send_message::<WsConnect>().await {
            Err(err) => bail!("Failed to send messsage on websocket, error: {}", err),
            _ => Ok(()),
        }
    }

    async fn post_office(mut receiver: Receiver<String>, cancel_token: CancellationToken) {
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    msg = receiver.recv() => {
                        match msg {
                            CoreResult::Ok(msg) => {
                                info!("{:?}", msg);
                                // Your further processing logic here
                            }
                            Err(RecvError::Lagged(err)) => warn!("websocket channel skipping a number of messages: {}", err),
                            Err(RecvError::Closed) => {
                                error!("websocket channel closed");
                                cancel_token.cancel()
                            }
                        };
                    }
                    _ = cancel_token.cancelled() => {
                        break
                    }
                }
            }
        });
    }

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
    use std::thread::sleep;
    use std::time::Duration;
    use std::{thread, time};

    #[derive(Debug, Serialize, Deserialize)]
    struct AuthResponse {
        rememberme: String,
        session_id: String,
    }

    #[tokio::test]
    async fn test_refresh_session_token() {
        let mut server = Server::new();
        let url = server.url();
        let auth_creds = &auth_token_response();
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
            .match_body(Matcher::JsonString(to_json(&auth_creds).unwrap()))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(to_json(&response).unwrap())
            .create();

        let client = WebClient::refresh_session_token(&client.http_client, auth_creds).await;
        assert!(client.is_ok());
        mock.assert();
    }

    #[ignore = "live-test"]
    #[tokio::test]
    async fn test_live_refresh_remember_token() {
        // Specify the URL you want to use for testing
        let config = env::var("OPTIONS_CFG")
            .expect("Failed to get the cfg file from the environment variable.");
        let settings = Config::read_config_file(&config).expect("Failed to parse config file");
        let db = DBClient::new(&settings).await.unwrap();

        let cancel_token = CancellationToken::new();
        let client = WebClient::new(&BASE_URL_UAT, &WS_URL_UAT, cancel_token.clone()).await;
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
                    CoreResult::Ok(val) => anyhow::Result::Ok(val),
                    Err(err) => {
                        let error = format!("{}", err);
                        anyhow::Result::Err(error)
                    }
                };
            assert!(refreshed.is_ok());

            let data = &refreshed.unwrap().data;
            let result = match WebClient::update_auth_from_db(
                &data.session,
                &data.remember,
                settings.endpoint,
                &db,
            )
            .await
            {
                CoreResult::Ok(val) => anyhow::Result::Ok(val),
                Err(err) => {
                    let error = format!("{}", err);
                    anyhow::Result::Err(error)
                }
            };

            assert!(result.is_ok());

            let second = time::Duration::from_secs(1);
            thread::sleep(second);
        }

        let first = stored_auths.clone().0;
        let second = stored_auths.clone().1;
        assert!(first.ne(&second));
    }
}
