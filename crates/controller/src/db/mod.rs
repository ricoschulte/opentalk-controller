//! Contains the database interface, ORM and database migrations
use crate::settings;
use diesel::pg::Pg;
use diesel::query_builder::{AstPass, Query, QueryFragment};
use diesel::query_dsl::LoadQuery;
use diesel::r2d2::ConnectionManager;
use diesel::result::Error;
use diesel::sql_types::BigInt;
use diesel::{r2d2, PgConnection, QueryResult, RunQueryDsl};
use std::time::Duration;

/// Allows to create one or more typed ids
///
/// Defines the type and implements a variety of traits for it to be usable with diesel.
/// See https://stackoverflow.com/a/59948116 for more information.
macro_rules! diesel_newtype {
    ($($name:ident($to_wrap:ty) => $sql_type:ty, $sql_type_lit:literal),+) => {
        $(
            pub use __newtype_impl::$name;
        )+

        mod __newtype_impl {
            use diesel::backend::Backend;
            use diesel::deserialize;
            use diesel::serialize::{self, Output};
            use diesel::types::{FromSql, ToSql};
            use serde::{Deserialize, Serialize};
            use std::io::Write;
            use std::fmt;

            $(

            #[derive(
                Debug,
                Clone,
                Copy,
                PartialEq,
                Eq,
                PartialOrd,
                Ord,
                Hash,
                Serialize,
                Deserialize,
                AsExpression,
                FromSqlRow,
            )]
            #[sql_type = $sql_type_lit]
            pub struct $name($to_wrap);

            impl $name {
                pub const fn from(inner: $to_wrap) -> Self {
                    Self (inner)
                }

                pub fn into_inner(self) -> $to_wrap {
                    self.0
                }
            }

            impl fmt::Display for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    self.0.fmt(f)
                }
            }

            impl<DB> ToSql<$sql_type, DB> for $name
            where
                DB: Backend,
                $to_wrap: ToSql<$sql_type, DB>,
            {
                fn to_sql<W: Write>(&self, out: &mut Output<W, DB>) -> serialize::Result {
                    <$to_wrap as ToSql<$sql_type, DB>>::to_sql(&self.0, out)
                }
            }

            impl<DB> FromSql<$sql_type, DB> for $name
            where
                DB: Backend,
                $to_wrap: FromSql<$sql_type, DB>,
            {
                fn from_sql(bytes: Option<&DB::RawValue>) -> deserialize::Result<Self> {
                    <$to_wrap as FromSql<$sql_type, DB>>::from_sql(bytes).map(Self)
                }
            }

            )+
        }
    };
}

pub mod groups;
pub mod legal_votes;
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
    #[tracing::instrument(skip(db_settings))]
    pub fn connect(db_settings: &settings::Database) -> Result<Self> {
        Self::connect_url(
            &db_settings.url,
            db_settings.max_connections,
            Some(db_settings.min_idle_connections),
        )
    }

    /// Creates a new DbInterface instance from the specified database url.
    pub fn connect_url(db_url: &str, max_conns: u32, min_idle: Option<u32>) -> Result<Self> {
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

    /// Returns an established connection from the connection pool
    fn get_con(&self) -> Result<DbConnection> {
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

pub trait Paginate: Sized {
    fn paginate(self, page: i64) -> Paginated<Self>;
    fn paginate_by(self, per_page: i64, page: i64) -> Paginated<Self>;
}

impl<T> Paginate for T {
    fn paginate(self, page: i64) -> Paginated<Self> {
        Paginated {
            query: self,
            per_page: DEFAULT_PER_PAGE,
            page,
        }
    }
    fn paginate_by(self, per_page: i64, page: i64) -> Paginated<Self> {
        Paginated {
            query: self,
            per_page,
            page,
        }
    }
}

const DEFAULT_PER_PAGE: i64 = 10;

#[derive(Debug, Clone, Copy, QueryId)]
pub struct Paginated<T> {
    query: T,
    page: i64,
    per_page: i64,
}

impl<T> Paginated<T> {
    pub fn per_page(self, per_page: i64) -> Self {
        Paginated { per_page, ..self }
    }

    pub fn load_and_count<U, Conn>(self, conn: &Conn) -> QueryResult<(Vec<U>, i64)>
    where
        Self: LoadQuery<Conn, (U, i64)>,
    {
        let results = self.load::<(U, i64)>(conn)?;
        let total = results.get(0).map(|x| x.1).unwrap_or(0);
        let records = results.into_iter().map(|x| x.0).collect();
        Ok((records, total))
    }
}

impl<T: Query> Query for Paginated<T> {
    type SqlType = (T::SqlType, BigInt);
}

impl<T> RunQueryDsl<PgConnection> for Paginated<T> {}

impl<T> QueryFragment<Pg> for Paginated<T>
where
    T: QueryFragment<Pg>,
{
    fn walk_ast(&self, mut out: AstPass<Pg>) -> QueryResult<()> {
        out.push_sql("SELECT *, COUNT(*) OVER () FROM (");
        self.query.walk_ast(out.reborrow())?;
        out.push_sql(") t LIMIT ");
        out.push_bind_param::<BigInt, _>(&self.per_page)?;
        out.push_sql(" OFFSET ");
        let offset = (self.page - 1) * self.per_page;
        out.push_bind_param::<BigInt, _>(&offset)?;
        Ok(())
    }
}
