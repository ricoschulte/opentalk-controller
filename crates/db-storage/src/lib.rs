//! Contains the database ORM and database migrations for the controller/storage
//! Builds upon k3k-database
//!
//! To extend you need to implement a fitting trait extension.
//! Example:
//! ```rust
//! # use anyhow::Result;
//! # use database::DbInterface;
//! pub trait DbExampleEx: DbInterface {
//!     fn create_user(&self, new_user: ()) -> Result<()> {
//!         let con = self.get_conn()?;
//!         // Do query and Return result
//!         Ok(())
//!     }
//! }
//! impl<T: DbInterface> DbExampleEx for T {}
//! ```

#[macro_use]
extern crate diesel;
#[macro_use]
mod macros;
mod schema;

pub mod groups;
pub mod invites;
pub mod legal_votes;
pub mod migrations;
pub mod rooms;
pub mod sip_configs;
pub mod users;

pub use database;
pub use rooms::DbRoomsEx;
pub use users::DbUsersEx;
