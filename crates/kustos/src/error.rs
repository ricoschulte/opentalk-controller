// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use std::num::ParseIntError;
use thiserror::Error;
use tokio::task::JoinError;

/// A combining error type which is returned by most major kustos methods
///
/// Derived using [`thiserror::Error`]
#[derive(Debug, Error)]
pub enum Error {
    #[error("Not Authorized")]
    NotAuthorized,
    #[error("Failed to convert string to type, {0}")]
    ParsingError(#[from] ParsingError),
    #[error("Failed to convert Resource to type, {0}")]
    ResourceParsingError(#[from] ResourceParseError),
    #[error("Blocking error, {0}")]
    BlockingError(#[from] JoinError),
    #[error("Casbin error {0}")]
    CasbinError(#[from] casbin::Error),
    #[error("Tried to start already running authz enforcer autoload")]
    AutoloadRunning,
    #[error("{0}")]
    Custom(String),
}

/// The error type returned by the underlying [`casbin`] [enforcer](crate::SyncedEnforcer) for
/// converting the public types to/from [`casbin`] compatible strings
///
/// Derived using [`thiserror::Error`]
#[derive(Debug, Error)]
pub enum ParsingError {
    #[error("Invalid access method: `{0}`")]
    InvalidAccessMethod(String),
    #[error("String was not a PolicyUser casbin string: `{0}`")]
    PolicyUser(String),
    #[error("String was not a PolicyInternalGroup casbin string: `{0}")]
    PolicyInternalGroup(String),
    #[error("String was not a PolicyOPGroup casbin string: `{0}`")]
    PolicyOPGroup(String),
    #[error("Failed to parse UUID")]
    Uuid(#[from] uuid::Error),
    #[error("Custom: {0}")]
    Custom(String),
}

/// The error returned when a resource failed to be parsed
///
/// Currently supported types are only uuids and integers, all other use the fallback Other variant.
#[derive(Debug, Error)]
pub enum ResourceParseError {
    #[error("Failed to parse UUID")]
    Uuid(#[from] uuid::Error),
    #[error(transparent)]
    ParseInt(#[from] ParseIntError),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// A default specialized Result type for kustos
pub type Result<T, E = Error> = std::result::Result<T, E>;
