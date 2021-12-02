use anyhow::{bail, Context, Result};
use openidconnect::core::{self, CoreClient, CoreIdToken};
use openidconnect::url::Url;
use openidconnect::{
    AccessToken, AccessTokenHash, AdditionalProviderMetadata, AuthorizationCode, ClientId,
    CsrfToken, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier,
    ProviderMetadata, RedirectUrl, RefreshToken, Scope, TokenResponse,
};
use regex::Regex;
use reqwest::redirect::Policy;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

type KeycloakMetadata = ProviderMetadata<
    KeyCloakExtraMetadata,
    core::CoreAuthDisplay,
    core::CoreClientAuthMethod,
    core::CoreClaimName,
    core::CoreClaimType,
    core::CoreGrantType,
    core::CoreJweContentEncryptionAlgorithm,
    core::CoreJweKeyManagementAlgorithm,
    core::CoreJwsSigningAlgorithm,
    core::CoreJsonWebKeyType,
    core::CoreJsonWebKeyUse,
    core::CoreJsonWebKey,
    core::CoreResponseMode,
    core::CoreResponseType,
    core::CoreSubjectIdentifierType,
>;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct KeyCloakExtraMetadata {
    end_session_endpoint: Url,
}

impl AdditionalProviderMetadata for KeyCloakExtraMetadata {}

/// Contains the OIDC providers metadata
#[derive(Debug, Clone)]
pub struct OpenIdConnectContext {
    oidc_client: CoreClient,
    end_session_endpoint: Url,
}

/// Creates a new [`OpenIdConnectContext`] by discovering the OIDC providers metadata
pub async fn discover(
    issuer_url: &Url,
    client_id: &str,
    redirect_url: &Url,
) -> Result<OpenIdConnectContext> {
    let issuer_url = IssuerUrl::from_url(issuer_url.clone());
    let redirect_url = RedirectUrl::from_url(redirect_url.clone());
    let client_id = ClientId::new(client_id.to_string());

    let provider_metadata =
        KeycloakMetadata::discover_async(issuer_url, openidconnect::reqwest::async_http_client)
            .await?;
    let end_session_endpoint = provider_metadata
        .additional_metadata()
        .end_session_endpoint
        .clone();

    let oidc_client = CoreClient::from_provider_metadata(provider_metadata, client_id, None)
        .set_redirect_uri(redirect_url);

    Ok(OpenIdConnectContext {
        oidc_client,
        end_session_endpoint,
    })
}

#[derive(Debug)]
struct SessionChallenge {
    state: CsrfToken,
    nonce: Nonce,
    verifier: PkceCodeVerifier,
}

#[derive(Deserialize, Debug)]
struct ChallengeResponse {
    state: CsrfToken,
    code: AuthorizationCode,

    // TODO Not sure what keycloak provides this for
    #[allow(dead_code)]
    session_state: CsrfToken,
}

/// The session tokens
///
/// The `id token` is used on login at the controller.
///
/// The `access token` will be sent as a header on endpoints that require authentication.
///
/// The `refresh token` can be used to refresh the session tokens.
#[derive(Debug)]
pub struct SessionTokens {
    pub id_token: CoreIdToken,
    pub access_token: AccessToken,
    pub refresh_token: RefreshToken,
}

/// Parses the keycloak login page
///
/// Ensures all field names are as expected. Returns the url for the login action.
fn parse_keycloak_login(raw_page: &str) -> Result<Url> {
    //panic on bad regex pattern only
    let form_action_pattern = Regex::new(r#"(?s).*<form[^>]*action="([^"]*)"[^>]*>.*"#).unwrap();
    let action = if let Some(match_res) = form_action_pattern.captures(raw_page) {
        //if matched there must be a location
        let action = match_res.get(1).unwrap().as_str();
        let action = html_escape::decode_html_entities(action);
        Url::parse(&action)?
    } else {
        bail!(anyhow::Error::msg(
            "form action not found in keycloak login page"
        ));
    };

    Ok(action)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UserRepresentation {
    email: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
    username: Option<String>,
    email_verified: Option<bool>,
}

impl OpenIdConnectContext {
    pub fn logout_url(&self) -> &Url {
        &self.end_session_endpoint
    }

    /// Performs the OIDC authentication.
    pub async fn authenticate<U: AsRef<str>, P: AsRef<str>>(
        &self,
        username: U,
        password: P,
    ) -> Result<SessionTokens> {
        let (auth_url, challenge) = self.start_session();

        let http_client = reqwest::Client::builder()
            .cookie_store(true)
            .redirect(Policy::none())
            .build()
            .unwrap();

        let login_response = http_client.get(auth_url).send().await?;
        anyhow::ensure!(login_response.status() == StatusCode::OK);
        let login_page = login_response.text().await?;

        let login_action = parse_keycloak_login(login_page.as_str())?;

        let params = [
            ("username", username.as_ref()),
            ("password", password.as_ref()),
            ("credentialId", ""),
        ];

        let login_result_resp = http_client.post(login_action).form(&params).send().await?;

        if login_result_resp.status() != StatusCode::FOUND {
            let status = login_result_resp.status();
            anyhow::bail!("HTTP STATUS is: {}", status);
        } else {
            let auth_finish_location = Url::parse(
                login_result_resp
                    .headers()
                    .get("location")
                    .expect("Redirect location unset")
                    .to_str()?,
            )?;
            let auth_finish_query = auth_finish_location
                .query()
                .expect("No challenge response in handshake");
            let response = serde_urlencoded::from_str(auth_finish_query);
            let response = match response {
                Ok(r) => r,
                Err(e) => {
                    log::error!("Error {}, Response was: {}", e, auth_finish_query);
                    return Err(e.into());
                }
            };
            let tokens = self.finish(response, challenge).await?;

            Ok(tokens)
        }
    }

    fn start_session(&self) -> (Url, SessionChallenge) {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();

        let (auth_url, csrf_token, nonce) = self
            .oidc_client
            .authorize_url(
                core::CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .add_scope(Scope::new("openid".to_string()))
            .add_scope(Scope::new("email".to_string()))
            .add_scope(Scope::new("roles".to_string()))
            .set_pkce_challenge(challenge)
            .url();

        let challenge = SessionChallenge {
            state: csrf_token,
            nonce,
            verifier,
        };

        (auth_url, challenge)
    }

    async fn finish(
        &self,
        response: ChallengeResponse,
        challenge: SessionChallenge,
    ) -> Result<SessionTokens> {
        anyhow::ensure!(challenge.state.secret() == response.state.secret());

        let token_response = self
            .oidc_client
            .exchange_code(response.code)
            .set_pkce_verifier(challenge.verifier)
            .request_async(openidconnect::reqwest::async_http_client)
            .await
            .context("Failed to exchange authentication code for access token")?;

        let id_token = token_response
            .id_token()
            .context("Token response did not contain an ID token")?;

        let claims = id_token
            .claims(&self.oidc_client.id_token_verifier(), &challenge.nonce)
            .context("Failed to verify ID token claims")?;

        if let Some(expected_access_token_hash) = claims.access_token_hash() {
            let actual_access_token_hash = AccessTokenHash::from_token(
                token_response.access_token(),
                &id_token
                    .signing_alg()
                    .context("Failed to get a supported signing algorithm")?,
            )?;

            if actual_access_token_hash != *expected_access_token_hash {
                bail!("Invalid access token");
            }
        } else {
            bail!("No access token");
        }

        Ok(SessionTokens {
            id_token: id_token.to_owned(),
            access_token: token_response.access_token().clone(),
            refresh_token: token_response
                .refresh_token()
                .context("No Refresh token!")?
                .clone(),
        })
    }

    /// Refresh the session tokens
    pub async fn refresh_tokens(&mut self, token: RefreshToken) -> Result<SessionTokens> {
        let response = self
            .oidc_client
            .exchange_refresh_token(&token)
            .request_async(openidconnect::reqwest::async_http_client)
            .await
            .context("Failed to refresh tokens")?;

        Ok(SessionTokens {
            id_token: response
                .id_token()
                .cloned()
                .context("Missing ID Token in refresh")?,
            access_token: response.access_token().to_owned(),
            refresh_token: response.refresh_token().cloned().unwrap_or(token),
        })
    }
}
