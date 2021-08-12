use crate::settings::Database;
use actix_web::rt;
use anyhow::{Context, Result};
use refinery::include_migration_mods;
use refinery_core::tokio_postgres::{Config, NoTls};
use tokio::sync::oneshot;
use tracing::Instrument;

include_migration_mods!("src/db/migrations");

#[tracing::instrument(skip(config))]
async fn migrate(config: Config) -> Result<()> {
    log::debug!("config: {:?}", config);

    let (mut client, conn) = config
        .connect(NoTls)
        .await
        .context("Unable to connect to database")?;

    let (tx, rx) = oneshot::channel();

    rt::spawn(
        async move {
            if let Err(e) = conn.await {
                log::error!("connection error: {}", e)
            }

            tx.send(()).expect("Channel unexpectedly dropped");
        }
        .instrument(tracing::Span::current()),
    );

    // The runner is specified through the `include_migration_mods` macro
    runner().run_async(&mut client).await?;

    drop(client);

    // wait for the connection to close
    rx.await?;

    Ok(())
}

pub async fn migrate_from_settings(db_config: &Database) -> Result<()> {
    let mut config = Config::new();

    config
        .dbname(&db_config.name)
        .user(&db_config.user)
        .password(&db_config.password)
        .host(&db_config.server)
        .port(db_config.port);

    migrate(config).await
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    use anyhow::{Context, Result};

    /// Tests the refinery database migration.
    /// A database url has to be specified via the environment variable DATABASE_URL.
    ///
    /// If no environment variable is provided, the database url will default to:
    /// ```
    /// postgres://postgres:password123@localhost:5432/k3k
    /// ```
    #[actix_rt::test]
    async fn test_migration() -> Result<()> {
        let url = std::env::var("DATABASE_URL")
            .unwrap_or("postgres://postgres:password123@localhost:5432/k3k".to_string());

        migrate(url.parse()?).await.context("Migration failed")?;

        Ok(())
    }
}
