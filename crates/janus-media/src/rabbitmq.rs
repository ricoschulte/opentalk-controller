use controller::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    StartedTalking(ParticipantId),
    StoppedTalking(ParticipantId),
    RequestMute(RequestMute),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct RequestMute {
    /// The issuer of the mute request
    pub issuer: ParticipantId,
    /// Flag to determine if the mute shall be forced or not
    pub force: bool,
}
