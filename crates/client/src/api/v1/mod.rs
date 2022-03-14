use crate::K3KSession;
use reqwest::{Error, Request, Response, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt;

pub mod auth;
pub mod invites;
pub mod rooms;
pub mod users;

pub type Result<T> = std::result::Result<T, ApiError>;

#[derive(Debug)]
pub struct HttpError {
    /// Response status code
    pub status: StatusCode,
    /// Response body
    pub reason: String,
    pub request_id: String,
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "(error {}: {}, requestId: {})",
            self.status, self.reason, self.request_id
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// Connection error
    #[error("Connection error: {0}")]
    ConnectionError(String),
    /// URL parsing error
    #[error("Url error: {0}")]
    InvalidUrl(String),
    /// Reqwest error
    #[error("Reqwest error: {0}")]
    ReqwestError(String),
    /// A Non-200 HTTP response
    #[error("Http error: {0:?}: {1}")]
    NonSuccess(Box<Request>, HttpError),
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
    Ok(response.json::<T>().await?)
}

impl K3KSession {
    fn url(&self, url: &str) -> Result<Url> {
        self.config
            .k3k_url
            .join(url)
            .map_err(|e| ApiError::InvalidUrl(e.to_string()))
    }

    async fn execute_request(&self, request: Request) -> Result<Response> {
        let response = self
            .http_client
            .execute(request.try_clone().unwrap())
            .await?;

        if !response.status().is_success() {
            let request_id = response
                .headers()
                .get("X-Request-Id")
                .and_then(|s| s.to_str().map(|s| s.to_owned()).ok())
                .unwrap_or_else(|| "n/a".to_string());

            return Err(ApiError::NonSuccess(
                Box::new(request),
                HttpError {
                    status: response.status(),
                    reason: response.text().await?,
                    request_id,
                },
            ));
        }
        Ok(response)
    }

    async fn get_authenticated(&self, path: &str) -> Result<Response> {
        let url = self.url(path)?;
        let request = self
            .http_client
            .get(url)
            .header(
                "Authorization",
                format!("Bearer {}", self.tokens.access_token.secret()),
            )
            .build()?;

        self.execute_request(request).await
    }

    async fn post_json_authenticated<T>(&self, path: &str, data: &T) -> Result<Response>
    where
        T: Serialize,
    {
        let url = self.url(path)?;
        let request = self
            .http_client
            .post(url)
            .json(data)
            .header(
                "Authorization",
                format!("Bearer {}", self.tokens.access_token.secret()),
            )
            .build()?;

        self.execute_request(request).await
    }

    async fn put_json_authenticated<T>(&self, path: &str, data: &T) -> Result<Response>
    where
        T: Serialize,
    {
        let url = self.url(path)?;
        let request = self
            .http_client
            .put(url)
            .json(data)
            .header(
                "Authorization",
                format!("Bearer {}", self.tokens.access_token.secret()),
            )
            .build()?;

        self.execute_request(request).await
    }

    async fn delete_authenticated(&self, path: &str) -> Result<Response> {
        let url = self.url(path)?;
        let request = self
            .http_client
            .delete(url)
            .header(
                "Authorization",
                format!("Bearer {}", self.tokens.access_token.secret()),
            )
            .build()?;

        self.execute_request(request).await
    }
}
