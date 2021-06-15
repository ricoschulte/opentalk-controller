use std::collections::HashSet;

use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::k3k::api::v1::{parse_json_response, ApiError};
use crate::k3k::K3KSession;

/// The JSON Body expected when making a *POST* request on `/v1/auth/login`
#[derive(Debug, Serialize)]
pub struct Login {
    pub id_token: String,
}

impl Login {
    pub fn new(id_token: String) -> Self {
        Login { id_token }
    }
}

/// JSON Body of the response coming from the *POST* request on `/v1/auth/login/`
#[derive(Debug, Deserialize)]
pub struct LoginResponse {
    /// Permissions is a set of strings that each define a permission a user has.
    pub permissions: HashSet<String>,
}

impl K3KSession {
    /// Calls *POST '/v1/auth/login/*
    // maybe implement this as non-associated function
    pub async fn login(&self) -> Result<LoginResponse, ApiError> {
        let login = Login::new(self.tokens.id_token.to_string());

        let response = self
            .post_json_authenticated("/v1/auth/login", &login)
            .await?;

        parse_json_response(response).await
    }
}

/// Represents an OIDC Provider
#[derive(Debug, Deserialize, Eq, PartialEq, Hash)]
pub struct OidcProvider {
    pub name: String,
    pub url: Url,
}

/// A set of OIDC Providers
///
/// JSON Body of the response for *GET '/auth/login/*
#[derive(Debug, Deserialize)]
pub struct OidcProviders {
    pub providers: Vec<OidcProvider>,
}

/// Calls *GET '/v1/auth/login/*
pub async fn get_oidc_provider(k3k_url: &Url) -> Result<OidcProviders, ApiError> {
    let url = k3k_url
        .join("/v1/auth/login")
        .map_err(|e| ApiError::InvalidUrl(e.to_string()))?;

    let response = reqwest::get(url).await?;

    parse_json_response(response).await
}
