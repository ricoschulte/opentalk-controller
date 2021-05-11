//! Handles the application settings via a config file and environment variables.

use config::{Config, ConfigError, Environment, File};
use openidconnect::{ClientId, ClientSecret, IssuerUrl};
use serde::Deserialize;

/// Contains the application settings.
///
/// The application settings are set with a TOML config file. Settings specified in the config file
/// can be overwritten by environment variables. To do so, set an environment variable
/// with the prefix `K3K_CTRL_` followed by the field names you want to set. Fields are separated by an underscore `_`.
/// ```
/// K3K_CTRL_<field>_<field-of-field>...
/// ```
/// # Example
///
/// set the `database.server` field:
/// ```
/// K3K_CTRL_DATABASE_SERVER=localhost
/// ```
/// However, the field names in the environment variables are not allowed to have underscores.
/// So the field 'database.max_connections' would resolve to:
/// ```
/// K3K_CTRL_DATABASE_MAXCONNECTIONS=5
/// ```
/// # Note
/// Fields set via environment variables do not affect the underlying config file.
#[derive(Debug, Deserialize)]
pub struct Settings {
    pub database: Database,
    pub oidc: Oidc,
    pub http: Http,
}

impl Settings {
    /// Creates a new Settings instance from the provided TOML file.
    /// Specific fields can be set or overwritten with environment variables (See struct level docs for more details).
    pub fn load(file_name: &str) -> Result<Self, ConfigError> {
        let mut cfg = Config::new();

        cfg.merge(File::with_name(file_name))?;

        let env = Environment::with_prefix("K3K_CTRL").separator("_");

        cfg.merge(env)?;

        cfg.try_into()
    }
}

#[derive(Debug, Deserialize)]
pub struct Database {
    pub server: String,
    pub port: u32,
    pub name: String,
    #[serde(rename = "maxconnections")]
    pub max_connections: u32,
    pub user: String,
    pub password: String,
}

/// Settings for OpenID Connect protocol which is used for user management.
#[derive(Debug, Deserialize)]
pub struct Oidc {
    pub provider: OidcProvider,
}

/// Information about the OIDC Provider
#[derive(Debug, Deserialize)]
pub struct OidcProvider {
    pub issuer: IssuerUrl,
    pub client_id: ClientId,
    pub client_secret: ClientSecret,
}

#[derive(Debug, Deserialize)]
pub struct Http {
    // TODO ADD TLS SETTINGS
    #[serde(default = "default_http_port")]
    pub port: u16,
    #[serde(default)]
    pub cors: Cors,
}

/// Settings for CORS (Cross Origin Resource Sharing)
#[derive(Default, Clone, Debug, Deserialize)]
pub struct Cors {
    #[serde(default)]
    pub allowed_origin: Vec<String>,
}

fn default_http_port() -> u16 {
    80
}
