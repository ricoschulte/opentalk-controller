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

    tokio::spawn(
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

pub async fn migrate_from_url(url: &str) -> Result<()> {
    let config = url.parse::<Config>()?;
    migrate(config).await
}

#[cfg(test)]
mod migration_tests {
    use anyhow::Result;

    /// Tests the refinery database migration.
    /// A database config has to be specified via the environment variables
    /// * POSTGRES_BASE_URL (default: `postgres://postgres:password123@localhost:5432`) - url to the postgres database without the database name specifier
    /// * DATABASE_NAME (default: `k3k_test`) - the database name inside postgres
    #[tokio::test]
    async fn test_migration() -> Result<()> {
        // This will create a database and migrate it
        test_util::database::DatabaseContext::new(false).await;

        Ok(())
    }
}