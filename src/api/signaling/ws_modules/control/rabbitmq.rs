use crate::api::signaling::ParticipantId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Message {
    Joined(ParticipantId),
    Left(ParticipantId),
    Update(ParticipantId),
}
