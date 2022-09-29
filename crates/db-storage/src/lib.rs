#![allow(clippy::extra_unused_lifetimes)]

//! Contains the database ORM and database migrations for the controller/storage
//! Builds upon k3k-database
//!
//! To extend you need to implement a fitting trait extension.
//! Example:
//! ```rust
//! # use anyhow::Result;
//! # use diesel::PgConnection;
//! trait DbFeatureExt {
//!     fn feature_a(&self, x: bool) -> Result<bool>;
//! }
//! impl DbFeatureExt for PgConnection {
//!     fn feature_a(&self, x: bool) -> Result<bool> {
//!         // Do stuff with self and x
//!         Ok(true)
//!     }
//! }
//! ```

#[macro_use]
extern crate diesel;

// postgres functions
use diesel::sql_types::{Integer, Text};

#[macro_use]
mod macros;
mod schema;

pub mod events;
pub mod groups;
pub mod invites;
pub mod legal_votes;
pub mod migrations;
pub mod rooms;
pub mod sip_configs;
pub mod users;
pub mod utils;

sql_function!(fn lower(x: Text) -> Text);
sql_function!(fn levenshtein(x: Text, y: Text) -> Integer);
sql_function!(fn soundex(x: Text) -> Text);

// SQL types reexport for schema.rs
pub mod sql_types {
    pub use super::events::EventExceptionKindType as Event_exception_kind;
    pub use super::events::EventInviteStatusType as Event_invite_status;
    pub use diesel::sql_types::*;
}
