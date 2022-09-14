use serde::Serialize;
use url::Url;

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "message")]
pub enum Message {
    SpaceUrl(AccessUrl),
    PdfUrl(AccessUrl),
    Error(Error),
}

#[derive(Debug, PartialEq, Serialize)]
pub struct AccessUrl {
    pub url: Url,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "error")]
pub enum Error {
    /// The requesting user has insufficient permissions for the operation
    InsufficientPermissions,
    /// Is send when another instance is currently initializing spacedeck
    CurrentlyInitializing,
    /// The spacedeck initialization failed
    InitializationFailed,
    /// Spacedeck is already initialized
    AlreadyInitialized,
}
