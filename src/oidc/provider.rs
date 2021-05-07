use crate::settings;
use anyhow::{Context, Result};
use openidconnect::core::CoreClient;
use openidconnect::reqwest::async_http_client as http_client;
use openidconnect::url::Url;
use openidconnect::{ClientId, IntrospectionUrl};
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
    pub async fn discover(provider: settings::OidcProvider) -> Result<ProviderClient> {
        let metadata = ProviderMetadata::discover_async(provider.issuer, http_client)
            .await
            .context("Failed to discover provider metadata")?;

        let client = CoreClient::new(
            provider.client_id.clone(),
            Some(provider.client_secret),
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
            client_id: provider.client_id,
            client,
        })
    }
}
