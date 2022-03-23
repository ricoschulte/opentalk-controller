use controller_shared::ParticipantId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    /// Generate an access url for the current etherpad
    GenerateUrl(GenerateUrl),
}

/// A receiving participant shall generate an access url.
///
/// The participant shall generate a write- or readonly-url considering the
/// provided writer list.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GenerateUrl {
    /// A list of participants that get write access
    pub writer: Vec<ParticipantId>,
}
