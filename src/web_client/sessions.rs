use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::sync::RwLock;
use tokio::sync::broadcast::Sender;
use url::Url;

use super::ApiQuoteToken;

pub trait WebSocketSession {
    fn api_token(&self) -> Option<ApiQuoteToken>;
    fn url(&self) -> Url;
    fn auth_token(&self) -> &mut String;
    fn session_id(&self) -> &mut String;
    fn to_ws(&self) -> &Sender<String>;
    fn is_alive(&self) -> &mut bool;
}

#[derive(Clone, Debug)]
pub struct AccountSession {
    url: Url,
    auth_token: String,
    session_id: String,
    last: DateTime<Utc>,
    to_ws: Sender<String>,
    is_alive: bool,
}

impl WebSocketSession for AccountSession {
    fn api_token(&self) -> Option<ApiQuoteToken> {
        None
    }

    fn url(&self) -> Url {
        self.url
    }

    fn auth_token(&mut self) -> &mut String {
        self.auth_token
    }

    fn session_id(&mut self) -> &mut String {
        self.session_id
    }

    fn to_ws(&self) -> &Sender<String> {
        self.to_ws
    }

    fn is_alive(&mut self) -> &mut bool {
        self.is_alive
    }
}

impl AccountSession {
    pub fn new(url: &str, to_ws: Sender<String>) -> Arc<RwLock<AccountSession>> {
        Arc::new(RwLock::new(AccountSession {
            url: Url::parse(url).unwrap(),
            session_id: String::default(),
            auth_token: String::default(),
            last: Utc::now(),
            to_ws,
            is_alive: false,
        }))
    }
}

#[derive(Clone, Debug)]
pub struct MktdataSession {
    api_quote_token: ApiQuoteToken,
    last: DateTime<Utc>,
    to_ws: Sender<String>,
    is_alive: bool,
}

impl MktdataSession {
    pub fn new(
        api_quote_token: ApiQuoteToken,
        to_ws: Sender<String>,
    ) -> Arc<RwLock<MktdataSession>> {
        Arc::new(RwLock::new(MktdataSession {
            api_quote_token,
            last: Utc::now(),
            to_ws,
            is_alive: false,
        }))
    }
}

impl WebSocketSession for MktdataSession {
    fn api_token(&self) -> Option<ApiQuoteToken> {
        self.api_quote_token
    }

    fn url(&self) -> Url {
        Url::default()
    }

    fn auth_token(&self) -> Option<String> {
        None
    }

    fn session_id(&self) -> &mut String {
        self.session_id
    }

    fn to_ws(&self) -> &Sender<String> {
        self.to_ws
    }

    fn is_alive(&self) -> bool {
        self.is_alive
    }
}
