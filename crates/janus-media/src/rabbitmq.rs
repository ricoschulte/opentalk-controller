use controller::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    StartedTalking(ParticipantId),
    StoppedTalking(ParticipantId),
}
