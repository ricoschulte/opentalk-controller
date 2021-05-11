#![allow(dead_code)] //TODO: remove this when some module implements the interface
use crate::settings;
use anyhow::{Context, Result};
use rbatis::core::db::DBPoolOptions;
use rbatis::crud::{CRUDTable, CRUD};
use rbatis::rbatis::Rbatis;
use std::fmt::Debug;
use std::time::Duration;

/// A database interface that wraps `Rbatis` (ORM framework).
///
/// This wrapper utilizes the `CRUDTable` trait from `Rbatis` for database operations.
#[derive(Debug)]
pub struct DbInterface {
    inner: Rbatis,
    cfg: settings::Database,
}

impl DbInterface {
    /// Creates a new DbInterface instance with specified database settings.
    pub fn new(cfg: settings::Database) -> Self {
        let rb = Rbatis::new();

        Self { inner: rb, cfg }
    }

    /// Connects to the configured database. Uses the database settings which were provided on instantiation.
    pub async fn connect(&self) -> Result<()> {
        let driver_url = build_driver_url(&self.cfg);

        let mut pool_options = DBPoolOptions::new();
        pool_options.max_connections = self.cfg.max_connections;
        pool_options.connect_timeout = Duration::from_secs(10);

        self.inner
            .link_opt(&driver_url, &pool_options)
            .await
            .context("Unable to connect to database")?;

        Ok(())
    }

    /// Insert an entity into the database.
    ///
    /// Returns the amount of rows affected
    pub async fn insert<T>(&self, entity: &T) -> Result<u64>
    where
        T: CRUDTable + Debug,
    {
        let exec_result = self
            .inner
            .save("", entity)
            .await
            .with_context(|| format!("Unable to insert database entity '{:?}'", entity))?;

        Ok(exec_result.rows_affected)
    }

    /// Select a record by ID.
    ///
    /// To recognise the id type and affected table, you have to specify the implementor or the
    /// CRUDTable trait.
    ///
    /// # Example:
    /// ```
    /// db_interface.select_by_id::<User>(&uuid).await;
    /// ```
    ///
    /// Return Ok(None) if no entity was found
    pub async fn select_by_id<T>(&self, id: &T::IdType) -> Result<Option<T>>
    where
        T: CRUDTable,
    {
        Ok(self
            .inner
            .fetch_by_id::<Option<T>>("", id)
            .await
            .with_context(|| format!("Unable to select database entity by id '{}'", id))?)
    }

    /// Delete a record from the database.
    ///
    /// To recognise the id type and affected table, you have to specify the implementor or the
    /// CRUDTable trait.
    ///
    /// # Example:
    /// ```
    /// db_interface.remove_by_id::<User>(&uuid).await;
    /// ```
    ///
    /// Returns the amount of rows affected
    pub async fn remove_by_id<T>(&self, id: &T::IdType) -> Result<u64>
    where
        T: CRUDTable,
    {
        Ok(self
            .inner
            .remove_by_id::<T>("", id)
            .await
            .with_context(|| format!("Unable to remove database entity by id '{}'", id))?)
    }

    /// Update a record in the database.
    ///
    /// Returns the amount of rows affected
    pub async fn update_by_id<T>(&self, entity: &mut T) -> Result<u64>
    where
        T: CRUDTable + Debug,
    {
        Ok(self
            .inner
            .update_by_id::<T>("", entity)
            .await
            .with_context(|| {
                format!(
                    "Unable to update database entity with the provided entity: '{:?}'",
                    entity
                )
            })?)
    }
}

fn build_driver_url(cfg: &settings::Database) -> String {
    format!(
        "postgres://{}:{}@{}:{}/{}",
        cfg.user, cfg.password, cfg.server, cfg.port, cfg.name
    )
}
