use crate::settings::Database;
use actix_web::rt;
use anyhow::{Context, Result};
use refinery::include_migration_mods;
use refinery_core::tokio_postgres::{connect, NoTls};

include_migration_mods!("src/db/migrations");

async fn start_migration_from_url(url: String) -> Result<()> {
    let (mut client, conn) = connect(&url, NoTls)
        .await
        .context("Unable to connect to database")?;

    rt::spawn(async move {
        if let Err(e) = conn.await {
            log::error!("connection error: {}", e)
        }
    });

    // The runner is specified through the `include_migration_mods` macro
    runner().run_async(&mut client).await?;

    Ok(())
}

pub async fn start_migration(db_config: &Database) -> Result<()> {
    let connection_config = format!(
        "host={} port={} dbname={} user={} password={}",
        db_config.server, db_config.port, db_config.name, db_config.user, db_config.password
    );

    start_migration_from_url(connection_config).await
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

        start_migration_from_url(url)
            .await
            .context("Migration failed")?;

        Ok(())
    }
}
