use crate::api::v1::{parse_json_response, ApiError, Result};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The JSON Body expected when making a *POST* request on `/introspect`
#[derive(Debug, Serialize)]
pub struct IntrospectRequest {
    pub token: String,
    /// TypeHint
    ///
    /// Expected values: refresh_token, access_token
    pub token_type_hint: Option<String>,
}

/// The JSON Body returned on by `/introspect`
#[derive(Debug, Deserialize)]
pub struct IntrospectResponse {
    pub active: bool,
    pub sub: Option<Uuid>,
}

/// Calls *POST '/introspect*
pub async fn check_introspect(
    k3k_url: &Url,
    client: &Client,
    introspect_request: &IntrospectRequest,
) -> Result<IntrospectResponse> {
    let mut k3k_url = k3k_url.to_owned();
    k3k_url
        .set_port(Some(8844))
        .map_err(|_| ApiError::InvalidUrl("failed setting port for introspect url".to_string()))?;

    let url = k3k_url
        .join("/introspect")
        .map_err(|e| ApiError::InvalidUrl(e.to_string()))?;

    let response = client.post(url).json(introspect_request).send().await?;

    parse_json_response(response).await
}
