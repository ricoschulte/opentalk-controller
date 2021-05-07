//! Handles the application settings via a config file and environment variables.
use config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    pub database: Database,
}

impl Settings {
    /// Creates a new Settings instance from the provided TOML file.
    /// Specific fields can be set or overwritten with environment variables (See struct level docs for more details).
    pub fn new(file: &str) -> Result<Self, ConfigError> {
        let mut cfg = Config::new();

        cfg.merge(File::with_name(file))?;

        let env = Environment::with_prefix("K3K_CTRL").separator("_");

        cfg.merge(env)?;

        cfg.try_into()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Database {
    pub server: String,
    pub port: u32,
    pub name: String,
    #[serde(rename = "maxconnections")]
    pub max_connections: u32,
    pub user: String,
    pub password: String,
}
