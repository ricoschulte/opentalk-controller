use std::rc::Rc;

use anyhow::Result;
use reqwest::redirect::Policy;
use reqwest::{Client, Url};

use crate::k3k::api::v1::auth::get_oidc_provider;
use crate::keycloak::{self, OpenIdContext, SessionTokens};

pub mod api;

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

pub fn default_config() -> Config {
    Config {
        k3k_url: Url::parse("http://localhost:8000").expect("bad k3k_url {}"),
        client_id: "Frontend".to_string(),
        //redirect_url: Url::parse("urn:ietf:wg:oauth:2.0:oob").expect("bad redirect_url {}"),
        redirect_url: Url::parse("http://localhost:8081/auth/keycloak/sso")
            .expect("bad redirect_url {}"),
    }
}

impl Config {
    /// Discovers the OIDC providers metadata
    ///
    /// Creates a new [`OpenIdContext`]
    pub async fn openid_discover(&self) -> Result<OpenIdContext> {
        let providers = get_oidc_provider(&self.k3k_url).await?;
        anyhow::ensure!(providers.providers.len() == 1);
        let issuer_url = &providers.providers[0].url;
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
