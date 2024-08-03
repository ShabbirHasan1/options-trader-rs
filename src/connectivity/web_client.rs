use anyhow::bail;
use anyhow::Ok;
use anyhow::Result;
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

use crate::connectivity::{http_client::HttpClient, sessions::*, web_socket::WebSocketClient};
use crate::platform::positions::OptionType;
use crate::platform::{db_client::*, settings::Settings};
use crate::tt_api::mktdata::{ApiQuoteToken, Wrapper};
use crate::tt_api::sessions::ConnectAccounts;
use crate::tt_api::sessions::ConnectMktData;

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
struct LoginCreds {
    #[serde(rename = "login")]
    username: String,
    password: String,
    #[serde(rename = "remember-me")]
    #[sqlx(default)]
    remember_me: bool,
}

#[derive(FromRow, Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
struct LoginAuthToken {
    #[serde(rename = "login")]
    username: String,
    #[serde(rename = "remember-token")]
    remember: String,
    #[serde(rename = "remember-me")]
    #[sqlx(default)]
    remember_me: bool,
}

#[derive(FromRow, Clone, PartialEq, Eq, Debug)]
struct DbStoredCreds {
    username: String,
    account: String,
    session: String,
    remember: String,
    #[sqlx(flatten)]
    endpoint: EndPoint,
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

const CHANNEL_CAPACITY_TO_WS: usize = 100;
const CHANNEL_CAPACITY_FROM_MD_WS: usize = 100;
const CHANNEL_CAPACITY_FROM_ACC_WS: usize = 50;

#[derive(Clone, Debug)]
pub struct WebClient {
    session: String,
    account: String,
    http_client: HttpClient,
    account_ws: Option<WebSocketClient<AccountSession>>,
    mktdata_ws: Option<WebSocketClient<MktdataSession>>,
    mktdata_session: Sender<String>,
    account_session: Sender<String>,
    cancel_token: CancellationToken,
}

impl WebClient {
    pub async fn new(base_url: &str, cancel_token: CancellationToken) -> Result<Self> {
        let (md_channel, _) = broadcast::channel::<String>(CHANNEL_CAPACITY_FROM_MD_WS);
        let (acc_channel, _) = broadcast::channel::<String>(CHANNEL_CAPACITY_FROM_ACC_WS);

        Ok(WebClient {
            session: String::default(),
            account: String::default(),
            http_client: HttpClient::new(&format!("https://{}", base_url)),
            account_ws: None,
            mktdata_ws: None,
            mktdata_session: md_channel,
            account_session: acc_channel,
            cancel_token,
        })
    }

    pub async fn startup(
        &mut self,
        account_session_url: &str,
        settings: Settings,
        db: &DBClient,
    ) -> Result<()> {
        let mut creds = Self::fetch_auth_from_db(settings.endpoint, db).await?;
        assert!(creds.len() == 1);
        let data = &mut creds[0];

        let password = std::env::var("TASTY_PASSWORD").ok();
        let updates =
            match Self::initialise_session(&self.http_client, data.clone(), password).await {
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
        self.account.clone_from(&data.account);

        let api_quote_token = self
            .get_api_quote_token(&self.http_client, &self.session)
            .await?;

        let (to_ws, _) = broadcast::channel::<String>(CHANNEL_CAPACITY_TO_WS);
        self.mktdata_ws = Some(
            self.subscribe_to_mktdata(api_quote_token, to_ws, self.cancel_token.clone())
                .await?,
        );

        info!("Session token {}", self.session.clone());

        let (to_ws, _) = broadcast::channel::<String>(CHANNEL_CAPACITY_TO_WS);
        self.account_ws = Some(
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

    pub async fn post<Data, Response>(&self, endpoint: &str, data: Data) -> Result<Response>
    where
        Data: Serialize + for<'a> Deserialize<'a>,
        Response: Serialize + for<'a> Deserialize<'a>,
    {
        self.http_client
            .post::<Data, Response>(endpoint, data, Some(&self.session))
            .await
    }

    pub async fn put<Data, Response>(&self, endpoint: &str, data: Data) -> Result<Response>
    where
        Data: Serialize + for<'a> Deserialize<'a>,
        Response: Serialize + for<'a> Deserialize<'a>,
    {
        self.http_client
            .put::<Data, Response>(endpoint, data, Some(&self.session))
            .await
    }

    pub fn get_account(&self) -> &str {
        &self.account
    }

    pub async fn subscribe_to_symbol(
        &self,
        symbol: &str,
        event_type: &[&str],
        option_type: OptionType,
    ) -> Result<()> {
        let client = self.mktdata_ws.as_ref().unwrap();
        client
            .get_session()
            .write()
            .await
            .subscribe(Some(symbol), event_type, option_type)
    }

    async fn fetch_auth_from_db(endpoint: EndPoint, db: &DBClient) -> Result<Vec<DbStoredCreds>> {
        let columns = vec!["username", "endpoint"];

        let stmt = SqlQueryBuilder::prepare_fetch_statement("tasty_auth", &columns);
        match sqlx::query_as::<_, DbStoredCreds>(&stmt)
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
            &["session", "remember", "endpoint"],
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

    async fn initialise_session(
        http_client: &HttpClient,
        data: DbStoredCreds,
        password: Option<String>,
    ) -> Result<Wrapper<AuthResponse>> {
        async fn init_session_with_password(
            username: &str,
            password: String,
            http_client: &HttpClient,
        ) -> Result<Wrapper<AuthResponse>> {
            let creds = LoginCreds {
                username: username.to_string(),
                password,
                remember_me: true,
            };
            let result = http_client
                .post::<LoginCreds, Wrapper<AuthResponse>>("sessions", creds, None)
                .await?;
            info!("Refresh token success, token: {:?}", result);
            Ok(result)
        }

        async fn init_session_with_token(
            username: &str,
            remember: &str,
            http_client: &HttpClient,
        ) -> Result<Wrapper<AuthResponse>> {
            let creds = LoginAuthToken {
                username: username.to_string(),
                remember: remember.to_string(),
                remember_me: true,
            };
            let result = http_client
                .post::<LoginAuthToken, Wrapper<AuthResponse>>("sessions", creds, None)
                .await?;
            info!("Refresh token success, token: {:?}", result);
            Ok(result)
        }

        match password {
            Some(password) => {
                init_session_with_password(&data.username, password, http_client).await
            }
            None => init_session_with_token(&data.username, &data.remember, http_client).await,
        }
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

    pub fn subscribe_md_events(&self) -> Receiver<String> {
        self.mktdata_session.subscribe()
    }

    pub fn subscribe_acc_events(&self) -> Receiver<String> {
        self.mktdata_session.subscribe()
    }

    async fn subscribe_to_account_updates(
        &mut self,
        url: &str,
        account_id: &str,
        auth_token: &str,
        to_ws: Sender<String>,
        cancel_token: CancellationToken,
    ) -> Result<WebSocketClient<AccountSession>> {
        let account_session = AccountSession::new(
            &format!("wss://{}", url),
            to_ws,
            self.account_session.clone(),
        );

        let auth = account_session
            .write()
            .await
            .startup(account_id, auth_token);

        let ws_client =
            WebSocketClient::<AccountSession>::new(account_session, cancel_token.clone())?;

        ws_client.subscribe_to_events().await?;
        ws_client.send_message::<ConnectAccounts>(auth).await?;
        Ok(ws_client)
    }

    async fn subscribe_to_mktdata(
        &mut self,
        api_quote_token: ApiQuoteToken,
        to_ws: Sender<String>,
        cancel_token: CancellationToken,
    ) -> Result<WebSocketClient<MktdataSession>> {
        let mktdata_session =
            MktdataSession::new(api_quote_token, to_ws, self.mktdata_session.clone());

        let auth = mktdata_session.write().await.startup().await;

        let ws_client = WebSocketClient::<MktdataSession>::new(mktdata_session, cancel_token)?;

        ws_client.subscribe_to_events().await?;
        ws_client.send_message::<ConnectMktData>(auth).await?;
        Ok(ws_client)
    }
}
