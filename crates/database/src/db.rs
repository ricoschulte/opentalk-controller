use controller_shared::settings;
use diesel::r2d2::ConnectionManager;
use diesel::{r2d2, PgConnection};
use std::time::Duration;

use crate::{DatabaseError, DbConnection, DbInterface};

type DbPool = r2d2::Pool<ConnectionManager<PgConnection>>;

/// Db container that uses a connection pool to hand out connections.
///
/// Uses an r2d2 connection pool to manage multiple established connections.
pub struct Db {
    pool: DbPool,
}

impl Db {
    /// Creates a new DbInterface instance from the specified database settings.
    #[tracing::instrument(skip(db_settings))]
    pub fn connect(db_settings: &settings::Database) -> crate::Result<Self> {
        Self::connect_url(
            &db_settings.url,
            db_settings.max_connections,
            Some(db_settings.min_idle_connections),
        )
    }

    /// Creates a new DbInterface instance from the specified database url.
    pub fn connect_url(db_url: &str, max_conns: u32, min_idle: Option<u32>) -> crate::Result<Self> {
        let manager = ConnectionManager::<PgConnection>::new(db_url);

        let pool = diesel::r2d2::Pool::builder()
            .max_size(max_conns)
            .min_idle(min_idle)
            .connection_timeout(Duration::from_secs(10))
            .build(manager)
            .map_err(|e| {
                log::error!("Unable to create database connection pool, {}", e);
                DatabaseError::R2D2Error(e.to_string())
            })?;

        Ok(Self { pool })
    }
}

impl DbInterface for Db {
    /// Returns an established connection from the connection pool
    fn get_conn(&self) -> crate::Result<DbConnection> {
        match self.pool.get() {
            Ok(con) => Ok(con),
            Err(e) => {
                let state = self.pool.state();
                let msg = format!(
                    "Unable to get connection from connection pool.
                            Error: {}
                            Pool State:
                                {:?}",
                    e, state
                );
                log::error!("{}", &msg);
                Err(DatabaseError::R2D2Error(msg))
            }
        }
    }
}
