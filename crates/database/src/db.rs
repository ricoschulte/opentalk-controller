// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::metrics::{DatabaseMetrics, MetricsConnection};
use crate::{DatabaseError, DbConnection};
use controller_shared::settings;
use diesel::r2d2::ConnectionManager;
use diesel::{r2d2, PgConnection};
use std::sync::Arc;
use std::time::Duration;

type DbPool = r2d2::Pool<ConnectionManager<PgConnection>>;

/// Db container that uses a connection pool to hand out connections
///
/// Uses an r2d2 connection pool to manage multiple established connections.
pub struct Db {
    metrics: Option<Arc<DatabaseMetrics>>,
    pool: DbPool,
}

impl Db {
    /// Creates a new Db instance from the specified database settings.
    #[tracing::instrument(skip(db_settings))]
    pub fn connect(db_settings: &settings::Database) -> crate::Result<Self> {
        Self::connect_url(
            &db_settings.url,
            db_settings.max_connections,
            Some(db_settings.min_idle_connections),
        )
    }

    /// Creates a new Db instance from the specified database url.
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

        Ok(Self {
            metrics: None,
            pool,
        })
    }

    /// Set the metrics to use for this database pool
    pub fn set_metrics(&mut self, metrics: Arc<DatabaseMetrics>) {
        self.metrics = Some(metrics);
    }

    /// Returns an established connection from the connection pool
    pub fn get_conn(&self) -> crate::Result<DbConnection> {
        let res = self.pool.get();
        let state = self.pool.state();

        if let Some(metrics) = &self.metrics {
            metrics
                .dbpool_connections
                .record(state.connections as u64, &[]);
            metrics
                .dbpool_connections_idle
                .record(state.idle_connections as u64, &[]);
        }

        match res {
            Ok(conn) => {
                let conn = MetricsConnection {
                    metrics: self.metrics.clone(),
                    conn,
                };

                Ok(conn)
            }
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
