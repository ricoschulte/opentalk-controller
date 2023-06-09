// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

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
use config::{Config, ConfigError, Environment, File, FileFormat};
use openidconnect::{ClientId, ClientSecret};
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

pub type SharedSettings = Arc<ArcSwap<Settings>>;

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub database: Database,
    pub keycloak: Keycloak,
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
    #[serde(default)]
    pub avatar: Avatar,
    #[serde(default)]
    pub metrics: Metrics,
    #[serde(default)]
    pub etherpad: Option<Etherpad>,

    #[serde(default)]
    pub spacedeck: Option<Spacedeck>,

    #[serde(default)]
    pub call_in: Option<CallIn>,

    #[serde(default)]
    pub defaults: Defaults,

    #[serde(default)]
    pub endpoints: Endpoints,

    pub minio: MinIO,

    #[serde(default)]
    pub tenants: Tenants,

    #[serde(default)]
    pub tariffs: Tariffs,

    #[serde(flatten)]
    pub extensions: HashMap<String, config::Value>,
}

impl Settings {
    /// Creates a new Settings instance from the provided TOML file.
    /// Specific fields can be set or overwritten with environment variables (See struct level docs for more details).
    pub fn load(file_name: &str) -> Result<Self, ConfigError> {
        Config::builder()
            .add_source(File::new(file_name, FileFormat::Toml))
            .add_source(
                Environment::with_prefix("K3K_CTRL")
                    .prefix_separator("_")
                    .separator("__"),
            )
            .build()?
            .try_deserialize()
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

/// Settings for Keycloak
#[derive(Debug, Clone, Deserialize)]
pub struct Keycloak {
    pub base_url: Url,
    pub realm: String,
    pub client_id: ClientId,
    pub client_secret: ClientSecret,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Http {
    #[serde(default = "default_http_port")]
    pub port: u16,
    #[serde(default)]
    pub tls: Option<HttpTls>,
}

impl Default for Http {
    fn default() -> Self {
        Self {
            port: default_http_port(),
            tls: None,
        }
    }
}

const fn default_http_port() -> u16 {
    11311
}

#[derive(Debug, Clone, Deserialize)]
pub struct HttpTls {
    pub certificate: PathBuf,
    pub private_key: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Logging {
    #[serde(default = "default_directives")]
    pub default_directives: Vec<String>,

    #[serde(default)]
    pub otlp_tracing_endpoint: Option<String>,

    #[serde(default = "default_service_name")]
    pub service_name: String,

    #[serde(default = "default_service_namespace")]
    pub service_namespace: String,

    #[serde(default)]
    pub service_instance_id: Option<String>,
}

impl Default for Logging {
    fn default() -> Self {
        Self {
            default_directives: default_directives(),
            otlp_tracing_endpoint: None,
            service_name: default_service_name(),
            service_namespace: default_service_namespace(),
            service_instance_id: None,
        }
    }
}

fn default_service_name() -> String {
    "k3k-controller".into()
}

fn default_service_namespace() -> String {
    "opentalk".into()
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
    #[serde(default = "rabbitmq_default_min_connections")]
    pub min_connections: u32,
    #[serde(default = "rabbitmq_default_max_channels")]
    pub max_channels_per_connection: u32,
    /// Mail sending is disabled when this is None
    #[serde(default)]
    pub mail_task_queue: Option<String>,

    /// Recording is disabled if this isn't set
    #[serde(default)]
    pub recording_task_queue: Option<String>,
}

impl Default for RabbitMqConfig {
    fn default() -> Self {
        Self {
            url: rabbitmq_default_url(),
            min_connections: rabbitmq_default_min_connections(),
            max_channels_per_connection: rabbitmq_default_max_channels(),
            mail_task_queue: None,
            recording_task_queue: None,
        }
    }
}

fn rabbitmq_default_url() -> String {
    "amqp://guest:guest@localhost:5672".to_owned()
}

fn rabbitmq_default_min_connections() -> u32 {
    10
}

fn rabbitmq_default_max_channels() -> u32 {
    100
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

#[derive(Clone, Debug, Deserialize)]
pub struct Etherpad {
    pub url: url::Url,
    pub api_key: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Spacedeck {
    pub url: url::Url,
    pub api_key: String,
}

fn duration_from_secs<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let duration: u64 = Deserialize::deserialize(deserializer)?;

    Ok(Duration::from_secs(duration))
}

#[derive(Clone, Debug, Deserialize)]
pub struct Avatar {
    #[serde(default = "default_libravatar_url")]
    pub libravatar_url: String,
}

impl Default for Avatar {
    fn default() -> Self {
        Self {
            libravatar_url: default_libravatar_url(),
        }
    }
}

fn default_libravatar_url() -> String {
    "https://seccdn.libravatar.org/avatar/".into()
}

#[derive(Clone, Debug, Deserialize)]
pub struct CallIn {
    pub tel: String,
    pub enable_phone_mapping: bool,
    pub default_country_code: phonenumber::country::Id,
}

#[derive(Clone, Default, Debug, Deserialize)]
pub struct Defaults {
    #[serde(default = "default_user_language")]
    pub user_language: String,
    #[serde(default)]
    pub screen_share_requires_permission: bool,
}

fn default_user_language() -> String {
    "en-US".into()
}

#[derive(Clone, Default, Debug, Deserialize)]
pub struct Endpoints {
    #[serde(default)]
    pub disable_users_find: bool,
    #[serde(default)]
    pub users_find_use_kc: bool,
    #[serde(default)]
    pub event_invite_external_email_address: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MinIO {
    pub uri: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct Metrics {
    pub allowlist: Vec<cidr::IpInet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantAssignment {
    Static { static_tenant_id: String },
    ByExternalTenantId,
}

impl Default for TenantAssignment {
    fn default() -> Self {
        Self::Static {
            static_tenant_id: String::from("OpenTalkDefaultTenant"),
        }
    }
}

#[derive(Default, Debug, Clone, Deserialize)]
pub struct Tenants {
    #[serde(default)]
    pub assignment: TenantAssignment,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TariffAssignment {
    Static { static_tariff_name: String },
    ByExternalTariffId,
}

impl Default for TariffAssignment {
    fn default() -> Self {
        Self::Static {
            static_tariff_name: String::from("OpenTalkDefaultTariff"),
        }
    }
}

#[derive(Default, Debug, Clone, Deserialize)]
pub struct Tariffs {
    #[serde(default)]
    pub assignment: TariffAssignment,
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::env;

    #[test]
    fn settings_env_vars_overwrite_config() -> Result<(), ConfigError> {
        // Sanity check
        let settings = Settings::load("../../extra/example.toml")?;

        assert_eq!(
            settings.database.url,
            "postgres://postgres:password123@localhost:5432/k3k"
        );
        assert_eq!(settings.http.port, 11311u16);

        // Set environment variables to overwrite default config file
        let env_db_url = "postgres://envtest:password@localhost:5432/opentalk".to_string();
        let env_http_port: u16 = 8000;
        let screen_share_requires_permission = true;
        env::set_var("K3K_CTRL_DATABASE__URL", &env_db_url);
        env::set_var("K3K_CTRL_HTTP__PORT", env_http_port.to_string());
        env::set_var(
            "K3K_CTRL_DEFAULTS__SCREEN_SHARE_REQUIRES_PERMISSION",
            screen_share_requires_permission.to_string(),
        );

        let settings = Settings::load("../../extra/example.toml")?;

        assert_eq!(settings.database.url, env_db_url);
        assert_eq!(settings.http.port, env_http_port);
        assert_eq!(
            settings.defaults.screen_share_requires_permission,
            screen_share_requires_permission
        );

        Ok(())
    }
}
