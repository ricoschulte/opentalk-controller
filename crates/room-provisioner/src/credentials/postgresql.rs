use super::helper::from_environment_with_default;
use anyhow::{Context, Result};
use controller::settings::Database;
use std::env;

pub fn from_environment() -> Result<Database> {
    let database = Database {
        url: url_from_environment()?,
        max_connections: from_environment_with_default::<u32>("DB_MAX_CONNECTIONS", 16)?,
        min_idle_connections: from_environment_with_default::<u32>("DB_MIN_IDLE_CONNECTIONS", 4)?,
    };
    Ok(database)
}

fn url_from_environment() -> Result<String> {
    let user = env::var("DB_USER").context("DB_USER not set")?;
    let password = env::var("DB_PASSWORD").context("DB_PASSWORD not set")?;
    let host = env::var("DB_HOST").unwrap_or_else(|_| "localhost".into());
    let port = from_environment_with_default::<u16>("DB_PORT", 5432)?;
    let name = env::var("DB_NAME").context("DB_NAME not set")?;
    Ok(format!(
        "postgres://{}:{}@{}:{}/{}",
        user, password, host, port, name
    ))
}
