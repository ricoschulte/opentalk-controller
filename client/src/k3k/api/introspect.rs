use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::k3k::api::v1::{parse_json_response, ApiError, Result};

/// The JSON Body expected when making a *POST* request on `/introspect`
#[derive(Debug, Serialize)]
pub struct IntrospectRequest {
    token: String,
    token_type_hint: Option<String>, // expected values: refresh_token, access_token
}

/// The JSON Body returned on by `/introspect`
#[derive(Debug, Deserialize)]
pub struct IntrospectResponse {
    active: bool,
    sub: Option<Uuid>,
}

/// Calls *GET '/introspect*
pub async fn check_introspect(
    k3k_url: &Url,
    client: &Client,
    introspect_request: &IntrospectRequest,
) -> Result<IntrospectResponse> {
    let url = k3k_url
        .join("/introspect")
        .map_err(|e| ApiError::InvalidUrl(e.to_string()))?;

    let response = client.get(url).json(introspect_request).send().await?;

    parse_json_response(response).await
}
