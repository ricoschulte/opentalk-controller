use super::http::async_http_client;
use anyhow::{anyhow, Context, Result};
use controller_shared::settings;
use openidconnect::core::CoreClient;
use openidconnect::url::Url;
use openidconnect::{ClientId, IntrospectionUrl, IssuerUrl};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AdditionalProviderMetadata {
    introspection_endpoint: Url,
}

impl openidconnect::AdditionalProviderMetadata for AdditionalProviderMetadata {}

type ProviderMetadata = openidconnect::ProviderMetadata<
    AdditionalProviderMetadata,
    openidconnect::core::CoreAuthDisplay,
    openidconnect::core::CoreClientAuthMethod,
    openidconnect::core::CoreClaimName,
    openidconnect::core::CoreClaimType,
    openidconnect::core::CoreGrantType,
    openidconnect::core::CoreJweContentEncryptionAlgorithm,
    openidconnect::core::CoreJweKeyManagementAlgorithm,
    openidconnect::core::CoreJwsSigningAlgorithm,
    openidconnect::core::CoreJsonWebKeyType,
    openidconnect::core::CoreJsonWebKeyUse,
    openidconnect::core::CoreJsonWebKey,
    openidconnect::core::CoreResponseMode,
    openidconnect::core::CoreResponseType,
    openidconnect::core::CoreSubjectIdentifierType,
>;

/// Contains all structures necessary to talk to a single configured OIDC Provider.
#[derive(Debug)]
pub struct ProviderClient {
    pub metadata: ProviderMetadata,
    pub client_id: ClientId,
    pub client: CoreClient,
}

impl ProviderClient {
    /// Discover Provider information from given settings
    pub async fn discover(
        http_client: reqwest::Client,
        config: settings::Keycloak,
    ) -> Result<ProviderClient> {
        let mut issuer_url = config.base_url.clone();
        issuer_url
            .path_segments_mut()
            .map_err(|_| anyhow!("keycloak base url cannot be a base"))?
            .extend(["realms", &config.realm]);

        let metadata = ProviderMetadata::discover_async(
            IssuerUrl::from_url(issuer_url),
            async_http_client(http_client),
        )
        .await
        .context("Failed to discover provider metadata")?;

        let client = CoreClient::new(
            config.client_id.clone(),
            Some(config.client_secret),
            metadata.issuer().clone(),
            metadata.authorization_endpoint().clone(),
            metadata.token_endpoint().cloned(),
            metadata.userinfo_endpoint().cloned(),
            metadata.jwks().clone(),
        )
        .set_introspection_uri(IntrospectionUrl::from_url(
            metadata
                .additional_metadata()
                .introspection_endpoint
                .clone(),
        ));

        Ok(ProviderClient {
            metadata,
            client_id: ClientId::new(config.client_id.into()),
            client,
        })
    }
}
