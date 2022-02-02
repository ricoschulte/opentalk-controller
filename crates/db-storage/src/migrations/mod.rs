use anyhow::{Context, Result};
use refinery::{embed_migrations, Report};
use refinery_core::tokio_postgres::{Config, NoTls};
use tokio::sync::oneshot;
use tracing::Instrument;

embed_migrations!(".");

#[tracing::instrument(skip(config))]
async fn migrate(config: Config) -> Result<Report> {
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
    let report = migrations::runner().run_async(&mut client).await?;

    drop(client);

    // wait for the connection to close
    rx.await?;

    Ok(report)
}

pub async fn migrate_from_url(url: &str) -> Result<Report> {
    let config = url.parse::<Config>()?;
    migrate(config).await
}

mod type_polyfills {
    use barrel::types::{BaseType, Type};

    /// An SQL datetime type
    ///
    /// Barrel 0.6.5 is missing datetime and 0.6.6 is not out yet, furthermore 0.6.6 only support TIMESTAMP which is without any timezone information
    pub fn datetime() -> Type {
        Type {
            nullable: false,
            unique: false,
            increments: false,
            indexed: false,
            primary: false,
            default: None,
            size: None,
            inner: BaseType::Custom("TIMESTAMPTZ"),
        }
    }
}
#[cfg(test)]
mod migration_tests {
    use anyhow::Result;
    use serial_test::serial;

    /// Tests the refinery database migration.
    /// A database config has to be specified via the environment variables
    /// * POSTGRES_BASE_URL (default: `postgres://postgres:password123@localhost:5432`) - url to the postgres database without the database name specifier
    /// * DATABASE_NAME (default: `k3k_test`) - the database name inside postgres
    #[tokio::test]
    #[serial]
    async fn test_migration() -> Result<()> {
        // This will create a database and migrate it
        test_util::database::DatabaseContext::new(false).await;

        Ok(())
    }
}
