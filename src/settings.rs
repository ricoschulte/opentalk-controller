//! Handles the application settings via a config file and environment variables.
use crate::cli::Args;
use arc_swap::ArcSwap;
use config::{Config, ConfigError, Environment, File};
use log::Level;
use openidconnect::{ClientId, ClientSecret, IssuerUrl};
use serde::{Deserialize, Deserializer};
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub type SharedSettings = Arc<ArcSwap<Settings>>;

/// Reload the settings from the `config_path` & the environment
///
/// Not all settings are used, as most of the settings are not reloadable while the
/// controller is running.
pub fn reload_settings(
    shared_settings: SharedSettings,
    config_path: &Path,
) -> Result<(), ConfigError> {
    let new_settings = Settings::load(config_path)?;
    let mut current_settings = (*shared_settings.load_full()).clone();

    // reload janus connection config
    current_settings.room_server.connections = new_settings.room_server.connections;

    // reload turn settings
    current_settings.turn = new_settings.turn;

    // replace the shared settings with the modified ones
    shared_settings.store(Arc::new(current_settings));

    Ok(())
}

/// Loads settings from [`Args`] and config file
///
/// The settings specified in the CLI-Arguments have a higher priority than the settings specified in the config file
pub fn load_settings(args: &Args) -> Result<Settings, ConfigError> {
    let mut settings = Settings::load(&args.config)?;

    if args.verbose > 0 {
        settings.logging.level = match args.verbose {
            0 => unreachable!(),
            1 => Level::Info,
            2 => Level::Debug,
            _ => Level::Trace,
        };
    }

    if let Some(log_output) = args.logoutput.clone() {
        settings.logging.file = if log_output == PathBuf::from("-") {
            None
        } else {
            Some(log_output)
        };
    }

    Ok(settings)
}

/// Contains the application settings.
///
/// The application settings are set with a TOML config file. Settings specified in the config file
/// can be overwritten by environment variables. To do so, set an environment variable
/// with the prefix `K3K_CTRL__` followed by the field names you want to set. Fields are separated by two underscores `__`.
/// ```sh
/// K3K_CTRL__<field>__<field-of-field>...
/// ```
///
/// # Example
///
/// set the `database.server` field:
/// ```sh
/// K3K_CTRL__DATABASE__SERVER=localhost
/// ```
///
/// However, the field names in the environment variables are not allowed to have underscores.
/// So the field 'database.max_connections' would resolve to:
/// ```sh
/// K3K_CTRL__DATABASE__MAX_CONNECTIONS=5
/// ```
/// # Note
/// Fields set via environment variables do not affect the underlying config file.
#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub database: Database,
    pub oidc: Oidc,
    pub http: Http,
    pub turn: Option<Turn>,
    pub redis: RedisConfig,
    pub rabbit_mq: RabbitMqConfig,
    pub room_server: JanusMcuConfig,
    #[serde(default = "default_logging")]
    pub logging: Logging,
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
    pub server: String,
    pub port: u32,
    pub name: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_min_idle_connections")]
    pub min_idle_connections: u32,
    pub user: String,
    pub password: String,
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
    #[serde(default = "default_log_level")]
    pub level: log::Level,
    #[serde(default)]
    pub file: Option<PathBuf>,
}

fn default_log_level() -> log::Level {
    Level::Warn
}

fn default_logging() -> Logging {
    Logging {
        level: default_log_level(),
        file: None,
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Turn {
    /// How long should a credential pair be valid, in seconds
    #[serde(deserialize_with = "duration_from_secs")]
    pub lifetime: chrono::Duration,
    /// List of configured TURN servers.
    pub servers: Vec<TurnServer>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TurnServer {
    // TURN URIs for this TURN server following rfc7065
    pub uris: Vec<String>,
    pub pre_shared_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedisConfig {
    #[serde(default = "redis_default_url")]
    pub url: url::Url,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JanusMcuConfig {
    pub connections: Vec<JanusRabbitMqConnection>,

    /// Max bitrate allowed for `video` media sessions
    #[serde(default = "default_max_video_bitrate")]
    pub max_video_bitrate: u64,

    /// Max bitrate allowed for `screen` media sessions
    #[serde(default = "default_max_screen_bitrate")]
    pub max_screen_bitrate: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RabbitMqConfig {
    #[serde(default = "rabbitmq_default_url")]
    pub url: String,
}

/// Take the settings from your janus rabbit mq transport configuration.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct JanusRabbitMqConnection {
    #[serde(default = "default_to_janus_routing_key")]
    pub to_janus_routing_key: String,
    #[serde(default = "default_janus_exchange")]
    pub janus_exchange: String,
    #[serde(default = "default_from_janus_routing_key")]
    pub from_janus_routing_key: String,
}

const fn default_max_video_bitrate() -> u64 {
    // 8kB/s
    64000
}

const fn default_max_screen_bitrate() -> u64 {
    // 1 MB/s
    8_000_000
}

const fn default_http_port() -> u16 {
    80
}

const fn internal_http_port() -> u16 {
    8844
}

fn default_max_connections() -> u32 {
    100
}

fn default_min_idle_connections() -> u32 {
    10
}

fn duration_from_secs<'de, D>(deserializer: D) -> Result<chrono::Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let duration: u64 = Deserialize::deserialize(deserializer)?;

    Ok(chrono::Duration::seconds(
        i64::try_from(duration).map_err(serde::de::Error::custom)?,
    ))
}

fn redis_default_url() -> url::Url {
    url::Url::try_from("redis://localhost:6379/").expect("Invalid default redis URL")
}

fn rabbitmq_default_url() -> String {
    "amqp://guest:guest@localhost:5672".to_owned()
}

fn default_to_janus_routing_key() -> String {
    "to-janus".to_owned()
}

fn default_janus_exchange() -> String {
    "janus-exchange".to_owned()
}

fn default_from_janus_routing_key() -> String {
    "from-janus".to_owned()
}

#[cfg(test)]
mod test {
    use super::Settings;
    use config::ConfigError;
    use std::path::Path;

    #[test]
    fn example_toml() -> Result<(), ConfigError> {
        let _settings = Settings::load(Path::new("extra/example.toml"))?;
        Ok(())
    }
}
