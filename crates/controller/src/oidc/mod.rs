use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use controller_shared::settings;
use http::async_http_client;
use openidconnect::{AccessToken, TokenIntrospectionResponse};
use provider::ProviderClient;

mod http;
mod jwt;
mod provider;

pub use jwt::VerifyError;

/// The `OidcContext` contains all information about the Oidc provider and permissions matrix.
#[derive(Debug)]
pub struct OidcContext {
    pub(crate) provider: ProviderClient,
    http_client: reqwest::Client,
}

impl OidcContext {
    /// Create the OidcContext from the configuration.
    /// This reads the OidcProvider configuration and tries to fetch the metadata from it.
    /// If a provider is misconfigured or not reachable this function will fail.
    #[tracing::instrument(name = "oidc_discover", skip(config))]
    pub async fn from_config(config: settings::Keycloak) -> Result<Self> {
        log::debug!("OIDC config: {:?}", config);

        let http_client = http::make_client()?;

        let client = ProviderClient::discover(http_client.clone(), config).await?;

        Ok(Self {
            provider: client,
            http_client,
        })
    }

    /// Verifies the signature and expiration of an AccessToken.
    ///
    /// Returns the subject (user id) if the token is verified.
    ///
    /// Note: This does __not__ check if the token is active or has been revoked.
    /// See `verify_access_token_active`.
    #[tracing::instrument(name = "oidc_verify_access_token", skip(self, access_token))]
    pub fn verify_access_token(
        &self,
        access_token: &AccessToken,
    ) -> Result<(String, String), VerifyError> {
        let claims = jwt::verify(
            self.provider.metadata.jwks(),
            access_token.secret().as_str(),
        )?;

        Ok((claims.iss, claims.sub))
    }

    /// Verify that an AccessToken is active using the providers `token_introspect` endpoint.
    ///
    /// Returns an error if it fails to validate the token.
    ///
    /// If the function returns Ok(_) the caller must inspect the returned [AccessTokenIntrospectInfo]
    /// to check if the AccessToken is still active.
    #[tracing::instrument(name = "oidc_introspect_access_token", skip(self, access_token), fields(active = tracing::field::Empty))]
    pub async fn introspect_access_token(
        &self,
        access_token: &AccessToken,
    ) -> Result<AccessTokenIntrospectInfo> {
        let response = self
            .provider
            .client
            .introspect(access_token)?
            .request_async(async_http_client(self.http_client.clone()))
            .await
            .context("Failed to verify token using the introspect endpoint")?;

        tracing::Span::current().record("active", &response.active());

        Ok(AccessTokenIntrospectInfo {
            active: response.active(),
        })
    }

    /// Verifies the signature and expiration of the ID Token and returns related info
    ///
    /// Returns an error if `id_token` is invalid or expired
    #[tracing::instrument(name = "oidc_verify_id_token", skip(self, id_token))]
    pub fn verify_id_token(&self, id_token: &str) -> Result<IdTokenInfo, VerifyError> {
        let claims = jwt::verify(self.provider.metadata.jwks(), id_token)?;

        Ok(IdTokenInfo {
            sub: claims.sub,
            issuer: claims.iss,
            expiration: claims.exp,
            email: claims.email,
            firstname: claims.given_name,
            lastname: claims.family_name,
            x_grp: claims.x_grp,
        })
    }

    pub fn provider_url(&self) -> String {
        self.provider.metadata.issuer().to_string()
    }
}

/// Relevant info returned from `verify_access_token_active` function.
#[derive(Debug)]
#[must_use]
pub struct AccessTokenIntrospectInfo {
    pub active: bool,
}

/// The result of an successful ID Token verification.
///
/// Contains the sub (client id) and expiration of the ID Token
#[derive(Debug)]
pub struct IdTokenInfo {
    pub sub: String,
    pub issuer: String,
    pub expiration: DateTime<Utc>,
    pub email: String,
    pub firstname: String,
    pub lastname: String,
    pub x_grp: Vec<String>,
}
