//! K3K Database connector, interface and connection handling

use diesel::pg::Pg;
use diesel::query_builder::{AstPass, Query, QueryFragment};
use diesel::query_dsl::LoadQuery;
use diesel::r2d2::ConnectionManager;
use diesel::result::Error;
use diesel::sql_types::BigInt;
use diesel::{r2d2, PgConnection, QueryResult, RunQueryDsl};

#[macro_use]
extern crate diesel;

mod db;
pub mod query_helper;

pub use db::Db;

/// Pooled connection alias
pub type DbConnection = r2d2::PooledConnection<ConnectionManager<PgConnection>>;

/// Result type using [`DatabaseError`] as a default Error
pub type Result<T, E = DatabaseError> = std::result::Result<T, E>;

/// Error types for the database abstraction
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Database Error: `{0}`")]
    Custom(String),
    #[error("Diesel Error: `{0}`")]
    DieselError(diesel::result::Error),
    #[error("A requested resource could not be found")]
    NotFound,
    // The R2D2 error mapping is only possible when using r2d2 directly as a dependency, hence the
    // generic R2D2 error handling. See https://github.com/diesel-rs/diesel/issues/2336
    #[error("The connection pool returned an Error: `{0}`")]
    R2D2Error(String),
}

pub trait OptionalExt<T, E> {
    fn optional(self) -> Result<Option<T>, E>;
}

impl<T> OptionalExt<T, DatabaseError> for Result<T, DatabaseError> {
    fn optional(self) -> Result<Option<T>, DatabaseError> {
        match self {
            Ok(t) => Ok(Some(t)),
            Err(DatabaseError::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

impl From<diesel::result::Error> for DatabaseError {
    fn from(err: diesel::result::Error) -> Self {
        match err {
            Error::NotFound => Self::NotFound,
            err => DatabaseError::DieselError(err),
        }
    }
}
/// Pagination trait for diesel
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

/// Paginated diesel database response
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
