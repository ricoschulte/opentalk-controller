//! Contains the database interface, ORM and database migrations
use crate::settings;
use diesel::r2d2::ConnectionManager;
use diesel::result::Error;
use diesel::{r2d2, PgConnection};
use std::time::Duration;

pub mod migrations;
pub mod rooms;
mod schema;
pub mod users;

pub(crate) type Result<T> = std::result::Result<T, DatabaseError>;

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Database Error: `{0}`")]
    Error(String),
    #[error("A requested resource could not be found")]
    NotFound,
    // The R2D2 error mapping is only possible when using r2d2 directly as a dependency, hence the
    // generic R2D2 error handling. See https://github.com/diesel-rs/diesel/issues/2336
    #[error("The connection pool returned an Error: `{0}`")]
    R2D2Error(String),
}

impl From<diesel::result::Error> for DatabaseError {
    fn from(err: diesel::result::Error) -> Self {
        match err {
            Error::NotFound => Self::NotFound,
            err => DatabaseError::Error(err.to_string()),
        }
    }
}

type DbPool = r2d2::Pool<ConnectionManager<PgConnection>>;
type DbConnection = r2d2::PooledConnection<ConnectionManager<PgConnection>>;

/// A database interface
///
/// Uses an r2d2 connection pool to manage multiple established connections.
pub struct DbInterface {
    pool: DbPool,
}

impl DbInterface {
    /// Creates a new DbInterface instance from the specified database settings.
    pub fn connect(db_settings: settings::Database) -> Result<Self> {
        let con_uri = pg_connection_uri(&db_settings);

        let manager = ConnectionManager::<PgConnection>::new(con_uri);

        let pool = diesel::r2d2::Pool::builder()
            .max_size(db_settings.max_connections)
            .min_idle(Some(db_settings.min_idle_connections))
            .connection_timeout(Duration::from_secs(10))
            .build(manager)
            .map_err(|e| DatabaseError::R2D2Error(e.to_string()))?;

        Ok(Self { pool })
    }

    /// Returns an established connection from the connection pool
    fn get_con(&self) -> Result<DbConnection> {
        match self.pool.get() {
            Ok(con) => Ok(con),
            Err(e) => {
                let state = self.pool.state();
                let msg = format!(
                    "Unable to receive connection from connection pool.
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

fn pg_connection_uri(cfg: &settings::Database) -> String {
    format!(
        "postgres://{}:{}@{}:{}/{}",
        cfg.user, cfg.password, cfg.server, cfg.port, cfg.name
    )
}
