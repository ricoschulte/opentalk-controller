/// Contains the application settings.
///
/// The application settings are set with a TOML config file. Settings specified in the config file
/// can be overwritten by environment variables. To do so, set an environment variable
/// with the prefix `K3K_CTRL_` followed by the field names you want to set. Nested fields are separated by two underscores `__`.
/// ```sh
/// K3K_CTRL_<field>__<field-of-field>...
/// ```
///
/// # Example
///
/// set the `database.url` field:
/// ```sh
/// K3K_CTRL_DATABASE__URL=postgres://postgres:password123@localhost:5432/k3k
/// ```
///
/// So the field 'database.max_connections' would resolve to:
/// ```sh
/// K3K_CTRL_DATABASE__MAX_CONNECTIONS=5
/// ```
///
/// # Note
///
/// Fields set via environment variables do not affect the underlying config file.
///
/// # Implementation Details:
///
/// Setting categories, in which all properties implement a default value, should also implement the [`Default`] trait.
///
use arc_swap::ArcSwap;
use config::{Config, ConfigError, Environment, File};
use openidconnect::{ClientId, ClientSecret, IssuerUrl};
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub type SharedSettings = Arc<ArcSwap<Settings>>;

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub database: Database,
    pub oidc: Oidc,
    pub http: Http,
    #[serde(default)]
    pub turn: Option<Turn>,
    #[serde(default)]
    pub stun: Option<Stun>,
    #[serde(default)]
    pub redis: RedisConfig,
    #[serde(default)]
    pub rabbit_mq: RabbitMqConfig,
    #[serde(default)]
    pub logging: Logging,
    #[serde(default)]
    pub authz: Authz,

    #[serde(flatten)]
    pub extensions: HashMap<String, config::Value>,
}

impl Settings {
    /// Creates a new Settings instance from the provided TOML file.
    /// Specific fields can be set or overwritten with environment variables (See struct level docs for more details).
    pub fn load(file_name: &Path) -> Result<Self, ConfigError> {
        let mut cfg = Config::new();

        cfg.merge(File::from(file_name))?;

        let env = Environment::with_prefix("K3K_CTRL").separator("__");

        cfg.merge(env)?;

        cfg.try_into()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Database {
    pub url: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_min_idle_connections")]
    pub min_idle_connections: u32,
}

fn default_max_connections() -> u32 {
    100
}

fn default_min_idle_connections() -> u32 {
    10
}

/// Settings for OpenID Connect protocol which is used for user management.
#[derive(Debug, Clone, Deserialize)]
pub struct Oidc {
    pub provider: OidcProvider,
}

/// Information about the OIDC Provider
#[derive(Debug, Clone, Deserialize)]
pub struct OidcProvider {
    pub issuer: IssuerUrl,
    pub client_id: ClientId,
    pub client_secret: ClientSecret,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Http {
    #[serde(default = "default_http_port")]
    pub port: u16,
    #[serde(default = "internal_http_port")]
    pub internal_port: u16,
    #[serde(default)]
    pub cors: HttpCors,
    #[serde(default)]
    pub tls: Option<HttpTls>,
}

impl Default for Http {
    fn default() -> Self {
        Self {
            port: default_http_port(),
            internal_port: internal_http_port(),
            cors: HttpCors::default(),
            tls: None,
        }
    }
}

const fn default_http_port() -> u16 {
    11311
}

const fn internal_http_port() -> u16 {
    8844
}

#[derive(Debug, Clone, Deserialize)]
pub struct HttpTls {
    pub certificate: PathBuf,
    pub private_key: PathBuf,
}

/// Settings for CORS (Cross Origin Resource Sharing)
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HttpCors {
    #[serde(default)]
    pub allowed_origin: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Logging {
    #[serde(default = "default_directives")]
    pub default_directives: Vec<String>,

    #[serde(default)]
    pub enable_opentelemetry: bool,

    #[serde(default = "default_service_name")]
    pub service_name: String,
}

impl Default for Logging {
    fn default() -> Self {
        Self {
            default_directives: default_directives(),
            enable_opentelemetry: false,
            service_name: default_service_name(),
        }
    }
}

fn default_service_name() -> String {
    "k3k-controller".into()
}

fn default_directives() -> Vec<String> {
    // Disable spamming noninformative traces
    vec![
        "k3k=INFO".into(),
        "pinky_swear=OFF".into(),
        "rustls=WARN".into(),
        "mio=ERROR".into(),
        "lapin=WARN".into(),
    ]
}

#[derive(Debug, Clone, Deserialize)]
pub struct Turn {
    /// How long should a credential pair be valid, in seconds
    #[serde(
        deserialize_with = "duration_from_secs",
        default = "default_turn_credential_lifetime"
    )]
    pub lifetime: Duration,
    /// List of configured TURN servers.
    pub servers: Vec<TurnServer>,
}

impl Default for Turn {
    fn default() -> Self {
        Self {
            lifetime: default_turn_credential_lifetime(),
            servers: vec![],
        }
    }
}

fn default_turn_credential_lifetime() -> Duration {
    Duration::from_secs(60)
}

#[derive(Debug, Clone, Deserialize)]
pub struct TurnServer {
    // TURN URIs for this TURN server following rfc7065
    pub uris: Vec<String>,
    pub pre_shared_key: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Stun {
    // STUN URIs for this TURN server following rfc7065
    pub uris: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedisConfig {
    #[serde(default = "redis_default_url")]
    pub url: url::Url,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: redis_default_url(),
        }
    }
}

fn redis_default_url() -> url::Url {
    url::Url::try_from("redis://localhost:6379/").expect("Invalid default redis URL")
}

#[derive(Debug, Clone, Deserialize)]
pub struct RabbitMqConfig {
    #[serde(default = "rabbitmq_default_url")]
    pub url: String,
}

impl Default for RabbitMqConfig {
    fn default() -> Self {
        Self {
            url: rabbitmq_default_url(),
        }
    }
}

fn rabbitmq_default_url() -> String {
    "amqp://guest:guest@localhost:5672".to_owned()
}

#[derive(Clone, Debug, Deserialize)]
pub struct Authz {
    /// Authz reload interval in seconds
    #[serde(
        deserialize_with = "duration_from_secs",
        default = "default_authz_reload_interval"
    )]
    pub reload_interval: Duration,
}

impl Default for Authz {
    fn default() -> Self {
        Self {
            reload_interval: default_authz_reload_interval(),
        }
    }
}

fn default_authz_reload_interval() -> Duration {
    Duration::from_secs(10)
}

fn duration_from_secs<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let duration: u64 = Deserialize::deserialize(deserializer)?;

    Ok(Duration::from_secs(duration))
}
