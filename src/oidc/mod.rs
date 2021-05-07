use crate::settings;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use openidconnect::reqwest::async_http_client as http_client;
use openidconnect::{AccessToken, TokenIntrospectionResponse};
use provider::ProviderClient;

mod jwt;
mod provider;

pub use jwt::VerifyError;

/// The `OidcContext` contains all information about the Oidc provider and permissions matrix.
#[derive(Debug)]
pub struct OidcContext {
    provider: ProviderClient,
}

impl OidcContext {
    /// Create the OidcContext from the configuration.
    /// This reads the OidcProvider configuration and tries to fetch the metadata from it.
    /// If a provider is misconfigured or not reachable this function will fail.
    pub async fn from_config(oidc_config: settings::Oidc) -> Result<Self> {
        let client = ProviderClient::discover(oidc_config.provider).await?;

        Ok(Self { provider: client })
    }

    /// Verifies the signature and expiration of an AccessToken.
    ///
    /// Returns the subject (user id) if the token is verified.
    ///
    /// Note: This does __not__ check if the token is active or has been revoked.
    /// See `verify_access_token_active`.
    pub fn verify_access_token(&self, access_token: &AccessToken) -> Result<String, VerifyError> {
        let claims = jwt::verify(
            self.provider.metadata.jwks(),
            access_token.secret().as_str(),
        )?;

        Ok(claims.sub)
    }

    /// Verify that an AccessToken is active using the providers `token_introspect` endpoint.
    ///
    /// Returns an error if it fails to validate the token.
    ///
    /// If the function returns Ok(_) the caller must inspect the returned [AccessTokenIntrospectInfo]
    /// to check if the AccessToken is still active.
    pub async fn introspect_access_token(
        &self,
        access_token: &AccessToken,
    ) -> Result<AccessTokenIntrospectInfo> {
        let response = self
            .provider
            .client
            .introspect(&access_token)?
            .request_async(http_client)
            .await
            .context("Failed to verify token using the introspect endpoint")?;

        Ok(AccessTokenIntrospectInfo {
            sub: response
                .sub()
                .context("introspect did not contain sub")?
                .into(),
            active: response.active(),
        })
    }

    /// Verifies the signature and expiration of the ID Token and returns related info
    ///
    /// Returns an error if `id_token` is invalid or expired
    pub async fn verify_id_token(&self, id_token: &str) -> Result<IdTokenInfo, VerifyError> {
        let claims = jwt::verify(self.provider.metadata.jwks(), id_token)?;

        Ok(IdTokenInfo {
            sub: claims.sub,
            expiration: claims.exp,
            x_grp: claims.x_grp,
        })
    }
}

/// Relevant info returned from `verify_access_token_active` function.
#[derive(Debug)]
#[must_use]
pub struct AccessTokenIntrospectInfo {
    pub sub: String,
    pub active: bool,
}

/// The result of an successful ID Token verification.
///
/// Contains the sub (client id) and expiration of the ID Token
#[derive(Debug)]
pub struct IdTokenInfo {
    pub sub: String,
    pub expiration: DateTime<Utc>,
    pub x_grp: Vec<String>,
}
