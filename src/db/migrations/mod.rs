use crate::settings::Database;
use anyhow::{Context, Result};
use refinery::include_migration_mods;
use refinery_core::tokio_postgres::{connect, NoTls};

include_migration_mods!("src/db/migrations");

pub async fn start_migration(db_config: &Database) -> Result<()> {
    let connection_config = format!(
        "host={} port={} dbname={} user={} password={}",
        db_config.server, db_config.port, db_config.name, db_config.user, db_config.password
    );

    let (mut client, conn) = connect(&connection_config, NoTls)
        .await
        .context("Unable to connect to database")?;

    tokio::spawn(async move {
        if let Err(e) = conn.await {
            log::error!("connection error: {}", e)
        }
    });

    // The runner is specified through the `include_migration_mods` macro
    runner().run_async(&mut client).await?;

    Ok(())
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    use anyhow::{Context, Result};
    use config::{Config, ConfigError, Environment};

    /// Tests the postgres database migration.
    /// All database settings have to be specified via environment variables.
    /// # Example
    /// ```
    /// K3K_CTRL_DATABASE_SERVER=localhost
    /// K3K_CTRL_DATABASE_PORT=5432
    /// K3K_CTRL_DATABASE_NAME=migration_test
    /// K3K_CTRL_DATABASE_MAXCONNECTIONS=1
    /// K3K_CTRL_DATABASE_USER=postgres
    /// K3K_CTRL_DATABASE_PASSWORD=password123
    /// ```
    #[tokio::test]
    async fn test_migration() -> Result<()> {
        let db_config = db_from_env()?;

        start_migration(&db_config)
            .await
            .context("Migration failed")?;

        Ok(())
    }

    /// Reads a Database config from environment variables where the prefix is `K3K_CTRL_DATABASE`.
    /// For example:
    /// ```
    /// K3K_CTRL_DATABASE_SERVER=localhost
    /// ```
    /// All database settings have to be specified.
    fn db_from_env() -> Result<Database, ConfigError> {
        let mut cfg = Config::new();
        let env = Environment::with_prefix("K3K_CTRL_DATABASE").separator("_");

        cfg.merge(env)?;

        cfg.try_into()
    }
}
