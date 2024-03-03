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
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::Sender;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::info;

pub(crate) mod http_client;
pub(crate) mod sessions;
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
        self.account = data.account.clone();

        let api_quote_token = self
            .get_api_quote_token(&self.http_client, &self.session)
            .await?;

        let (to_ws, _) = broadcast::channel::<String>(100);
        self.mktdata = Some(
            self.subscribe_to_mktdata(api_quote_token, to_ws, self.cancel_token.clone())
                .await?,
        );

        info!("Session token {}", self.session.clone());

        let (to_ws, _) = broadcast::channel::<String>(100);
        self.account_updates = Some(
            self.subscribe_to_account_updates(
                account_session_url,
                &data.account.clone(),
                &self.session.clone(),
                to_ws,
                self.cancel_token.clone(),
            )
            .await?,
        );

        Ok(())
    }

    pub async fn get<Response>(&self, endpoint: &str) -> Result<Response>
    where
        Response: Serialize + for<'a> Deserialize<'a>,
    {
        self.http_client
            .get::<Response>(endpoint, Some(&self.session))
            .await
    }

    pub fn get_account(&self) -> &str {
        &self.account
    }

    pub async fn subscribe_to_symbol(&self, symbol: &str) -> Result<()> {
        let mktdata = self.mktdata.as_ref().unwrap();
        let session = mktdata.get_session();
        let subscribe = session.write().await.subscribe(symbol);
        mktdata.send_message::<md_api::Channel>(subscribe).await
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
}
