// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use db_storage::assets::AssetId;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "message")]
pub enum Message {
    SpaceUrl(AccessUrl),
    PdfAsset(PdfAsset),
    Error(Error),
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct AccessUrl {
    pub url: Url,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfAsset {
    pub filename: String,
    pub asset_id: AssetId,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
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
