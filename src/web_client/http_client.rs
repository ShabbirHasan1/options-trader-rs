use anyhow::bail;
use anyhow::Ok;
use anyhow::Result;
use core::result::Result as CoreResult;
use flate2::read::GzDecoder;
use serde::Deserialize;
use serde::Serialize;
use serde_json::from_str as json_from_str;
use serde_json::to_string as to_json;
use std::future::IntoFuture;
use std::io;
use std::io::prelude::*;
use surf::http::headers;
use surf::Client;
use surf::RequestBuilder;
use tracing::info;
use url::Url;

#[derive(Clone, Debug)]
pub struct HttpClient {
    base_url: String,
    client: Client,
}

impl HttpClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            client: Client::new(),
        }
    }

    fn add_custom_headers(session: Option<&str>, request: RequestBuilder) -> RequestBuilder {
        let request = match session {
            Some(session) => request
                .header("Authorization", session)
                .header("product", "tasty-options-trader".to_string())
                .header("version", "0.1"),
            _ => request,
        };
        request.header("Content-Type", "application/json".to_string())
    }

    pub async fn get<Payload>(&self, endpoint: &str, session: Option<&str>) -> Result<Payload>
    where
        Payload: Serialize + for<'a> Deserialize<'a>,
    {
        let url = Url::parse(format!("{}/{}", self.base_url, endpoint).as_str())?;
        let mut response = match Self::add_custom_headers(session, self.client.get(url)).await {
            core::result::Result::Ok(val) => val,
            Err(err) => bail!("Failed get request, error: {}", err),
        };

        info!("request to endpoint: {}/{}", self.base_url, endpoint,);

        if !response.status().is_success() {
            bail!(
                "POST Request failed with status: {} text: {:?}",
                response.status(),
                response.body_string().await
            );
        }

        let mut d = GzDecoder::new("...".as_bytes());
        let mut s = String::new();
        d.read_to_string(&mut s).unwrap();
        println!("{}", s);
        info!("Response is {:?}", response);

        match response.body_json::<Payload>().await {
            surf::Result::Ok(val) => Ok(val),
            Err(err) => bail!(
                "Could not read json body: {}, error: {:?}",
                response.body_string().await.unwrap(),
                err
            ),
        }
    }

    pub async fn post<Payload, Response>(
        &self,
        endpoint: &str,
        data: Payload,
        session: Option<&str>,
    ) -> Result<Response>
    where
        Payload: Serialize + for<'a> Deserialize<'a>,
        Response: Serialize + for<'a> Deserialize<'a>,
    {
        let url = Url::parse(format!("{}/{}", self.base_url, endpoint).as_str())?;
        let payload = to_json(&data)?;
        info!(
            "request to endpoint: {}/{} with payload: {}",
            self.base_url, endpoint, payload
        );
        let builder =
            match Self::add_custom_headers(session, self.client.post(url)).body_json(&data) {
                core::result::Result::Ok(val) => val,
                Err(err) => bail!("Failed to post request {}", err),
            };

        let mut response = match builder.await {
            core::result::Result::Ok(val) => val,
            Err(err) => bail!("Failed to post request {}", err),
        };

        if !response.status().is_success() {
            bail!("POST Request failed with status: {}", response.status());
        }

        if let Some(encoding) = response.header("content-encoding") {
            if encoding.eq("gzip") {
                let bytes = match response.body_bytes().await {
                    surf::Result::Ok(val) => val,
                    Err(err) => bail!("Could not read bytes body, error: {}", err),
                };
                let mut decoder = GzDecoder::new(&bytes[..]);
                let mut output = String::new();
                let _ = decoder.read_to_string(&mut output);
                info!("GET Response body: {:?}", output);
                match json_from_str::<Response>(&output) {
                    CoreResult::Ok(val) => return anyhow::Result::Ok(val),
                    Err(err) => bail!("Failed to extract data from gzip encoding, error: {}", err),
                }
            }
        }
        info!("GET Response body: {:?}", response);
        match response.body_json::<Response>().await {
            surf::Result::Ok(val) => Ok(val),
            Err(err) => bail!("Could not read json body, error: {}", err),
        }
    }
}
