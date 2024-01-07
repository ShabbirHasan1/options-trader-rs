use anyhow::bail;
use anyhow::Ok;
use anyhow::Result;
use core::result::Result as CoreResult;
use reqwest::Client;
use reqwest::RequestBuilder;
use serde::Deserialize;
use serde::Serialize;
use serde_json::from_str as json_from_str;
use serde_json::to_string as to_json;
use tracing::info;
use url::Url;

pub struct HttpClient {
    base_url: String,
    client: Client,
    session: Option<String>,
}

impl HttpClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            client: Client::new(),
            session: None,
        }
    }

    fn add_custom_headers(session: &str, request: RequestBuilder) -> RequestBuilder {
        request
            .header("product", format!("tasty-options-trader"))
            .header("version", "0.1")
            .header("product", format!("application/json"))
            .header("Authorization", session)
    }

    pub async fn get<Payload>(&self, endpoint: &str, session: Option<&String>) -> Result<Payload>
    where
        Payload: Serialize + for<'a> Deserialize<'a>,
    {
        match &session {
            Some(session) => {
                let url = Url::parse(format!("{}/{}", self.base_url, endpoint).as_str())?;
                let response = Self::add_custom_headers(session, self.client.get(url))
                    .send()
                    .await?;

                if !response.status().is_success() {
                    bail!("POST Request failed with status: {}", response.status());
                }

                let body = response.text().await?;
                info!("POST Response body: {}", body);
                match json_from_str::<Payload>(&body) {
                    CoreResult::Ok(val) => Ok(val),
                    Err(err) => bail!("Failed to parse json on get request, error: {}", err),
                }
            }
            None => bail!("No session token"),
        }
    }

    pub async fn post<Payload, Response>(
        &self,
        endpoint: &str,
        data: Payload,
        session: Option<&String>,
    ) -> Result<Response>
    where
        Payload: Serialize + for<'a> Deserialize<'a>,
        Response: Serialize + for<'a> Deserialize<'a>,
    {
        let url = Url::parse(format!("{}/{}", self.base_url, endpoint).as_str())?;
        let payload = to_json(&data)?;
        info!(
            "request to endpoint: {} with payload: {}",
            endpoint, payload
        );
        let response = match &session {
            Some(session) => {
                Self::add_custom_headers(session, self.client.post(url))
                    .json(&data)
                    .send()
                    .await?
            }
            None => self.client.post(url).json(&data).send().await?,
        };

        if !response.status().is_success() {
            bail!("POST Request failed response: {:?}", response.text().await?);
        }

        let body = response.text().await?;
        info!("POST Response body: {}", body);
        match json_from_str::<Response>(&body) {
            CoreResult::Ok(val) => Ok(val),
            Err(err) => bail!("Failed to parse json on get request, error: {}", err),
        }
    }
}
