use crate::api::v1::auth::get_oidc_provider;
use crate::keycloak::{OpenIdConnectContext, SessionTokens};
use anyhow::Result;
use reqwest::redirect::Policy;
use reqwest::{Client, Url};
use std::rc::Rc;

pub mod api;
pub mod keycloak;

/// The session configuration
#[derive(Debug)]
pub struct Config {
    /// Controller URL
    pub k3k_url: Url,
    /// The client id e.g.: 'Frontend' or 'Testing'
    pub client_id: String,
    /// Redirect URL for the sso process
    pub redirect_url: Url,
}

impl Config {
    /// Discovers the OIDC providers metadata
    ///
    /// Creates a new [`OpenIdConnectContext`]
    pub async fn openid_connect_discover(&self) -> Result<OpenIdConnectContext> {
        let provider = get_oidc_provider(&self.k3k_url).await?;
        let issuer_url = &provider.oidc.url;
        keycloak::discover(issuer_url, &self.client_id, &self.redirect_url).await
    }
}

/// A session with the K3K controller
///
/// Is used to call the API endpoints on the controller.
#[derive(Debug)]
pub struct K3KSession {
    /// Reusable reqwest connection pool
    pub http_client: Client,
    /// Configuration for a K3K session
    pub config: Rc<Config>,
    /// The sessions id-, access- & refresh-token
    pub tokens: SessionTokens,
}

impl K3KSession {
    /// Creates a new session
    ///
    /// Attempts to login at the configured OIDC provider to acquire the session tokens.
    pub fn new(config: Rc<Config>, tokens: SessionTokens) -> Self {
        let http_client = reqwest::Client::builder()
            .cookie_store(true)
            .redirect(Policy::none())
            .build()
            .expect("could not create HTTP client");

        K3KSession {
            http_client,
            config,
            tokens,
        }
    }
}
