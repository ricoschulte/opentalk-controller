use diesel::connection::{
    AnsiTransactionManager, Connection, ConnectionGatWorkaround, DefaultLoadingMode,
    LoadConnection, LoadRowIter, SimpleConnection, TransactionManager,
};
use diesel::expression::QueryMetadata;
use diesel::query_builder::{Query, QueryFragment, QueryId};
use diesel::r2d2::{ConnectionManager, PooledConnection};
use diesel::result::{ConnectionResult, QueryResult};
use diesel::PgConnection;
use opentelemetry::metrics::{Counter, ValueRecorder};
use opentelemetry::Key;
use std::sync::Arc;
use std::time::Instant;

type Parent = PooledConnection<ConnectionManager<PgConnection>>;

const ERROR_KEY: Key = Key::from_static_str("error");

pub struct DatabaseMetrics {
    pub sql_execution_time: ValueRecorder<f64>,
    pub sql_error: Counter<u64>,
    pub dbpool_connections: ValueRecorder<u64>,
    pub dbpool_connections_idle: ValueRecorder<u64>,
}

pub struct MetricsConnection<Conn> {
    pub(crate) metrics: Option<Arc<DatabaseMetrics>>,
    pub(crate) conn: Conn,
}

impl<Conn> MetricsConnection<Conn> {
    fn instrument<F, T>(&mut self, f: F) -> Result<T, diesel::result::Error>
    where
        F: FnOnce(&mut Conn) -> Result<T, diesel::result::Error>,
    {
        if let Some(metrics) = &self.metrics {
            let start = Instant::now();

            match f(&mut self.conn) {
                res @ (Ok(_) | Err(diesel::result::Error::NotFound)) => {
                    metrics
                        .sql_execution_time
                        .record(start.elapsed().as_secs_f64(), &[]);

                    res
                }
                Err(e) => {
                    let labels = &[ERROR_KEY.string(get_metrics_label_for_error(&e))];
                    metrics.sql_error.add(1, labels);

                    Err(e)
                }
            }
        } else {
            f(&mut self.conn)
        }
    }
}

impl<'conn, 'query, Conn: Connection>
    ConnectionGatWorkaround<'conn, 'query, <Conn as Connection>::Backend>
    for MetricsConnection<Conn>
{
    type Cursor =
        <Conn as ConnectionGatWorkaround<'conn, 'query, <Conn as Connection>::Backend>>::Cursor;

    type Row = <Conn as ConnectionGatWorkaround<'conn, 'query, <Conn as Connection>::Backend>>::Row;
}

fn get_metrics_label_for_error(error: &diesel::result::Error) -> &'static str {
    match error {
        diesel::result::Error::InvalidCString(_) => "invalid_c_string",
        diesel::result::Error::DatabaseError(e, _) => match e {
            diesel::result::DatabaseErrorKind::UniqueViolation => "unique_violation",
            diesel::result::DatabaseErrorKind::ForeignKeyViolation => "foreign_key_violation",
            diesel::result::DatabaseErrorKind::UnableToSendCommand => "unable_to_send_command",
            diesel::result::DatabaseErrorKind::SerializationFailure => "serialization_failure",
            _ => "unknown",
        },
        diesel::result::Error::NotFound => unreachable!(),
        diesel::result::Error::QueryBuilderError(_) => "query_builder_error",
        diesel::result::Error::DeserializationError(_) => "deserialization_error",
        diesel::result::Error::SerializationError(_) => "serialization_error",
        diesel::result::Error::RollbackTransaction => "rollback_transaction",
        diesel::result::Error::AlreadyInTransaction => "already_in_transaction",
        _ => "unknown",
    }
}

impl<Conn> SimpleConnection for MetricsConnection<Conn>
where
    Conn: SimpleConnection,
{
    fn batch_execute(&mut self, query: &str) -> diesel::QueryResult<()> {
        self.instrument(|conn| conn.batch_execute(query))
    }
}

impl Connection for MetricsConnection<Parent> {
    type Backend = <Parent as Connection>::Backend;
    type TransactionManager = AnsiTransactionManager;

    fn establish(database_url: &str) -> ConnectionResult<Self> {
        Parent::establish(database_url).map(|conn| Self {
            metrics: None,
            conn,
        })
    }

    /// Execute a single SQL statements given by a query and return
    /// number of affected rows
    ///
    /// Hidden in `diesel` behind the
    /// `i-implement-a-third-party-backend-and-opt-into-breaking-changes` feature flag,
    /// therefore not generally visible in the `diesel` generated docs.
    fn execute_returning_count<T>(&mut self, source: &T) -> QueryResult<usize>
    where
        T: QueryFragment<Self::Backend> + QueryId,
    {
        self.instrument(|conn| conn.execute_returning_count(source))
    }

    /// Get access to the current transaction state of this connection
    ///
    /// Hidden in `diesel` behind the
    /// `i-implement-a-third-party-backend-and-opt-into-breaking-changes` feature flag,
    /// therefore not generally visible in the `diesel` generated docs.
    fn transaction_state(
        &mut self,
    ) -> &mut <Self::TransactionManager as TransactionManager<Self>>::TransactionStateData {
        self.conn.transaction_state()
    }
}

impl LoadConnection for MetricsConnection<Parent> {
    /// Executes a given query and returns any requested values
    ///
    /// Hidden in `diesel` behind the
    /// `i-implement-a-third-party-backend-and-opt-into-breaking-changes` feature flag,
    /// therefore not generally visible in the `diesel` generated docs.
    fn load<'conn, 'query, T>(
        &'conn mut self,
        source: T,
    ) -> QueryResult<LoadRowIter<'conn, 'query, Self, <Parent as Connection>::Backend>>
    where
        T: Query + QueryFragment<Self::Backend> + QueryId + 'query,
        Self::Backend: QueryMetadata<T::SqlType>,
    {
        <Parent as LoadConnection<DefaultLoadingMode>>::load(&mut self.conn, source)
    }
}
