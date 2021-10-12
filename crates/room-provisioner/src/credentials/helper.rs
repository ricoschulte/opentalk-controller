use anyhow::{Context, Result};
use std::{env, error::Error, str::FromStr};

pub fn from_environment_with_default<T: FromStr>(variable_name: &str, default: T) -> Result<T>
where
    <T as FromStr>::Err: Error + Send + Sync + 'static,
{
    env::var(variable_name)
        .map_or_else(|_| Ok(default), |value| value.parse::<T>())
        .with_context(|| format!("Could not parse {}", variable_name))
}
