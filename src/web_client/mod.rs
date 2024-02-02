use anyhow::bail;
use anyhow::Ok;
use anyhow::Result;
use chrono::DateTime;
use chrono::Duration;
use chrono::Utc;
use core::result::Result as CoreResult;
use serde::Deserialize;
use serde::Serialize;
use sqlx::postgres::PgRow;
use sqlx::FromRow;
use sqlx::Row;
use std::thread;

use tokio::sync::broadcast;

use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::Sender;

use tokio_util::sync::CancellationToken;
use tracing::debug;

use tracing::info;

mod http_client;
mod sessions;
mod tt_api;
mod websocket;

use crate::db_client::SqlQueryBuilder;

use self::sessions::acc_api;
use self::sessions::md_api;

use super::db_client::DBClient;
use super::settings::Settings;
use http_client::HttpClient;
use sessions::AccountSession;
use sessions::MktdataSession;
use websocket::WebSocketClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Wrapper<Response> {
    data: Response,
    context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApiQuoteToken {
    token: String,
    #[serde(rename = "streamer-url")]
    streamer_url: Option<String>,
    #[serde(rename = "websocket-url")]
    websocket_url: Option<String>,
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
    pub async fn new(base_url: &str, cancel_token: CancellationToken) -> Result<Self> {
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
        self.session = updates.data.session;
        self.account = updates.data.user.external_id;

        let api_quote_token = self
            .get_api_quote_token(&self.http_client, &self.session)
            .await?;

        let (to_ws, _) = broadcast::channel::<String>(100);
        // self.mktdata = Some(
        //     self.subscribe_to_mktdata(api_quote_token, to_ws.clone(), self.cancel_token.clone())
        //         .await?,
        // );

        self.account_updates = Some(
            self.subscribe_to_account_updates(
                account_session_url,
                &data.account.clone(),
                &self.session.clone(),
                to_ws.clone(),
                self.cancel_token.clone(),
            )
            .await?,
        );

        Ok(())
    }

    pub async fn send_message<Message>(&self, message: Message) -> Result<()>
    where
        Message: Serialize + for<'a> Deserialize<'a>,
    {
        if let Some(session) = &self.mktdata {
            session.send_message::<Message>(message).await?
        }
        bail!("System not ready, call startup first");
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
    ) -> Result<Wrapper<AuthResponse>> {
        data.remember_me = true;
        let refresh_token = http_client
            .post::<AuthCreds, Wrapper<AuthResponse>>("sessions", data, None)
            .await?;
        info!("Refresh token success, token: {:?}", refresh_token);
        Ok(refresh_token)
    }

    async fn get_api_quote_token(
        &self,
        http_client: &HttpClient,
        auth_token: &str,
    ) -> Result<ApiQuoteToken> {
        let response = http_client
            .get::<Wrapper<ApiQuoteToken>>("api-quote-tokens", Some(auth_token))
            .await?;
        Ok(response.data)
    }

    pub fn subscribe_to_events(&self) -> Receiver<String> {
        self.to_app.subscribe()
    }

    async fn subscribe_to_account_updates(
        &mut self,
        url: &str,
        account_id: &str,
        auth_token: &str,
        to_ws: Sender<String>,
        cancel_token: CancellationToken,
    ) -> Result<WebSocketClient<AccountSession>> {
        let account_session =
            AccountSession::new(&format!("wss://{}", url), to_ws, self.to_app.clone());

        let auth = account_session
            .write()
            .await
            .startup(account_id, auth_token)
            .await;

        let ws_client =
            WebSocketClient::<AccountSession>::new(account_session, cancel_token.clone())?;

        ws_client.subscribe_to_events().await?;
        ws_client.send_message::<acc_api::Connect>(auth).await?;
        Ok(ws_client)
    }

    async fn subscribe_to_mktdata(
        &mut self,
        api_quote_token: ApiQuoteToken,
        to_ws: Sender<String>,
        cancel_token: CancellationToken,
    ) -> Result<WebSocketClient<MktdataSession>> {
        let mktdata_session =
            MktdataSession::new(api_quote_token, to_ws.clone(), self.to_app.clone());

        let auth = mktdata_session.write().await.startup().await;

        let ws_client = WebSocketClient::<MktdataSession>::new(mktdata_session, cancel_token)?;

        ws_client.subscribe_to_events().await?;
        ws_client.send_message::<md_api::Connect>(auth).await?;
        Ok(ws_client)
    }

    //postman collects messages for the app from both sessions and channels to the rest
    // of the app - or maybe the session hold a sender given by client
    async fn post_office(mut receiver: Receiver<String>, cancel_token: CancellationToken) {
        tokio::spawn(async move {
            // loop {
            //     tokio::select! {
            //     //     msg = receiver.recv() => {
            //     //         match msg {
            //     //             CoreResult::Ok(msg) => {
            //     //                 info!("{:?}", msg);
            //     //                 // Your further processing logic here
            //     //             }
            //     //             Err(RecvError::Lagged(err)) => warn!("websocket channel skipping a number of messages: {}", err),
            //     //             Err(RecvError::Closed) => {
            //     //                 error!("websocket channel closed");
            //     //                 cancel_token.cancel()
            //     //             }
            //     //         };
            //     //     }
            //     //     _ = cancel_token.cancelled() => {
            //     //         break
            //     //     }
            //     // }
            // }
        });
    }

    async fn subscribe_to_mktdata_updates() {}
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     use mockito::Matcher;
//     use mockito::Server;
//     use tracing_subscriber::fmt::format;

//     use crate::settings::Config;
//     use crate::settings::DatabaseConfig;
//     use crate::settings::Settings;
//     use crate::utils::mock_db::MockDb;
//     use crate::utils::ws_server::WsServer;
//     use std::env;
//     use std::thread::sleep;
//     use std::time::Duration;
//     use std::{thread, time};

//     #[derive(Debug, Serialize, Deserialize)]
//     struct AuthResponse {
//         rememberme: String,
//         session_id: String,
//     }

//     #[tokio::test]
//     async fn test_refresh_session_token() {
//         let mut server = Server::new();
//         let url = server.url();
//         let auth_creds = &auth_token_response();
//         let client = WebClient::new(&url, &WS_URL_UAT).await;
//         assert!(client.is_ok());
//         let client = client.unwrap();

//         let response = AuthResponse {
//             rememberme: "905802385048509348503985".to_string(),
//             session_id: "098573495709283498724359872".to_string(),
//         };

//         // Create a mock
//         let mock = server
//             .mock("POST", "/sessions")
//             .match_header("content-type", "application/json")
//             .match_body(Matcher::JsonString(to_json(&auth_creds).unwrap()))
//             .with_status(201)
//             .with_header("content-type", "application/json")
//             .with_body(to_json(&response).unwrap())
//             .create();

//         let client = WebClient::refresh_session_token(&client.http_client, auth_creds).await;
//         assert!(client.is_ok());
//         mock.assert();
//     }

//     #[ignore = "live-test"]
//     #[tokio::test]
//     async fn test_live_refresh_remember_token() {
//         // Specify the URL you want to use for testing
//         let config = env::var("OPTIONS_CFG")
//             .expect("Failed to get the cfg file from the environment variable.");
//         let settings = Config::read_config_file(&config).expect("Failed to parse config file");
//         let db = DBClient::new(&settings).await.unwrap();

//         let cancel_token = CancellationToken::new();
//         let client = WebClient::new(&BASE_URL_UAT, &WS_URL_UAT, cancel_token.clone()).await;
//         assert!(client.is_ok());

//         let mut stored_auths: (AuthCreds, AuthCreds) = Default::default();
//         for index in 0..2 {
//             let auth_creds =
//                 WebClient::fetch_auth_from_db(&settings.username, settings.endpoint, &db).await;
//             assert!(auth_creds.is_ok());

//             let stored_data = match index {
//                 0 => {
//                     stored_auths.0 = auth_creds.unwrap()[0].clone();
//                     stored_auths.clone().0
//                 }

//                 1 => {
//                     stored_auths.1 = auth_creds.unwrap()[0].clone();
//                     stored_auths.clone().1
//                 }
//                 _ => panic!("Index out of range"),
//             };
//             let client = client.as_ref().unwrap();
//             let refreshed =
//                 match WebClient::refresh_session_token(&client.http_client, stored_data).await {
//                     CoreResult::Ok(val) => anyhow::Result::Ok(val),
//                     Err(err) => {
//                         let error = format!("{}", err);
//                         anyhow::Result::Err(error)
//                     }
//                 };
//             assert!(refreshed.is_ok());

//             let data = &refreshed.unwrap().data;
//             let result = match WebClient::update_auth_from_db(
//                 &data.session,
//                 &data.remember,
//                 settings.endpoint,
//                 &db,
//             )
//             .await
//             {
//                 CoreResult::Ok(val) => anyhow::Result::Ok(val),
//                 Err(err) => {
//                     let error = format!("{}", err);
//                     anyhow::Result::Err(error)
//                 }
//             };

//             assert!(result.is_ok());

//             let second = time::Duration::from_secs(1);
//             thread::sleep(second);
//         }

//         let first = stored_auths.clone().0;
//         let second = stored_auths.clone().1;
//         assert!(first.ne(&second));
//     }
// }
