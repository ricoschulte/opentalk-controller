use serde::{Deserialize, Serialize};

use crate::outgoing::PdfAsset;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    /// Spacedeck has been initialized
    Initialized,
    PdfAsset(PdfAsset),
}
