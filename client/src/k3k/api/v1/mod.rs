use std::fmt;

use reqwest::{Error, Response, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::k3k::K3KSession;

pub mod auth;
pub mod rooms;
pub mod users;

pub type Result<T> = std::result::Result<T, ApiError>;

#[derive(Debug)]
pub struct HttpError {
    status: StatusCode,
    reason: String,
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(error {}: {})", self.status, self.reason)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Connection error: {0}")]
    ConnectionError(String),
    #[error("Url error: {0}")]
    InvalidUrl(String),
    #[error("Reqwest error: {0}")]
    ReqwestError(String),
    #[error("Http error: {0}")]
    NonSuccess(HttpError),
}

impl From<reqwest::Error> for ApiError {
    fn from(e: Error) -> Self {
        Self::ReqwestError(e.to_string())
    }
}

pub(crate) async fn parse_json_response<T>(response: Response) -> Result<T>
where
    T: DeserializeOwned,
{
    if !response.status().is_success() {
        return Err(ApiError::NonSuccess(HttpError {
            status: response.status(),
            reason: response.text().await?,
        }));
    }
    Ok(response.json::<T>().await?)
}

impl K3KSession {
    fn url(&self, url: &str) -> Result<Url> {
        self.config
            .k3k_url
            .join(url)
            .map_err(|e| ApiError::InvalidUrl(e.to_string()))
    }

    async fn get_authenticated(&self, path: &str) -> Result<Response> {
        let url = self.url(path)?;
        let response = self
            .http_client
            .get(url)
            .header(
                "Authorization",
                format!("Bearer {}", self.tokens.access_token.secret()),
            )
            .send()
            .await?;

        Ok(response)
    }

    async fn post_json_authenticated<T>(&self, path: &str, data: &T) -> Result<Response>
    where
        T: Serialize,
    {
        let url = self.url(path)?;
        let response = self
            .http_client
            .post(url)
            .json(data)
            .header(
                "Authorization",
                format!("Bearer {}", self.tokens.access_token.secret()),
            )
            .send()
            .await?;

        Ok(response)
    }

    async fn put_json_authenticated<T>(&self, path: &str, data: &T) -> Result<Response>
    where
        T: Serialize,
    {
        let url = self.url(path)?;
        let response = self
            .http_client
            .put(url)
            .json(data)
            .header(
                "Authorization",
                format!("Bearer {}", self.tokens.access_token.secret()),
            )
            .send()
            .await?;

        Ok(response)
    }
}
