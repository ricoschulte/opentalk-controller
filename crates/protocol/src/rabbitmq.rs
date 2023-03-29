// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::outgoing::PdfAsset;
use serde::{Deserialize, Serialize};
use types::core::ParticipantId;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    /// Generate an access url for the current etherpad
    GenerateUrl(GenerateUrl),
    /// A pdf asset has been generated from the protocol
    PdfAsset(PdfAsset),
}

/// A receiving participant shall generate an access url.
///
/// The participant shall generate a write- or readonly-url considering the
/// provided writer list.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GenerateUrl {
    /// A list of participants that get write access
    pub writers: Vec<ParticipantId>,
}
