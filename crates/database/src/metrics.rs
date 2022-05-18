use diesel::connection::SimpleConnection;
use diesel::query_builder::{AsQuery, QueryFragment, QueryId};
use diesel::query_source::QueryableByName;
use diesel::r2d2::{ConnectionManager, PooledConnection};
use diesel::types::HasSqlType;
use diesel::{Connection, ConnectionResult, PgConnection, QueryResult, Queryable};
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
    fn instrument<F, T>(&self, f: F) -> QueryResult<T>
    where
        F: FnOnce(&Conn) -> QueryResult<T>,
    {
        if let Some(metrics) = &self.metrics {
            let start = Instant::now();

            match f(&self.conn) {
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
            f(&self.conn)
        }
    }
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
    fn batch_execute(&self, query: &str) -> diesel::QueryResult<()> {
        self.instrument(|conn| conn.batch_execute(query))
    }
}

impl Connection for MetricsConnection<Parent> {
    type Backend = <Parent as Connection>::Backend;
    type TransactionManager = <Parent as Connection>::TransactionManager;

    fn establish(database_url: &str) -> ConnectionResult<Self> {
        Parent::establish(database_url).map(|conn| Self {
            metrics: None,
            conn,
        })
    }

    fn execute(&self, query: &str) -> QueryResult<usize> {
        self.instrument(|conn| conn.execute(query))
    }

    fn query_by_index<T, U>(&self, source: T) -> QueryResult<Vec<U>>
    where
        T: AsQuery,
        T::Query: QueryFragment<Self::Backend> + QueryId,
        Self::Backend: HasSqlType<T::SqlType>,
        U: Queryable<T::SqlType, Self::Backend>,
    {
        self.instrument(|conn| conn.query_by_index(source))
    }

    fn query_by_name<T, U>(&self, source: &T) -> QueryResult<Vec<U>>
    where
        T: QueryFragment<Self::Backend> + diesel::query_builder::QueryId,
        U: QueryableByName<Self::Backend>,
    {
        self.instrument(|conn| conn.query_by_name(source))
    }

    fn execute_returning_count<T>(&self, source: &T) -> QueryResult<usize>
    where
        T: QueryFragment<Self::Backend> + QueryId,
    {
        self.instrument(|conn| conn.execute_returning_count(source))
    }

    fn transaction_manager(&self) -> &Self::TransactionManager {
        self.conn.transaction_manager()
    }
}
