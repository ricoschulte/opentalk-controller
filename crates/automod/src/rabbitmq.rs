//! Message types sent via rabbitmq.
//!
//! Mostly duplicates of [`super::outgoing`] types.
//! See their respective originals for documentation.
use super::config::FrontendConfig;
use controller_shared::ParticipantId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub enum Message {
    Start(Start),
    Stop,

    SpeakerUpdate(SpeakerUpdate),
    RemainingUpdate(RemainingUpdate),

    StartAnimation(StartAnimation),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Start {
    pub frontend_config: FrontendConfig,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SpeakerUpdate {
    pub speaker: Option<ParticipantId>,
    pub history: Option<Vec<ParticipantId>>,
    pub remaining: Option<Vec<ParticipantId>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RemainingUpdate {
    pub remaining: Vec<ParticipantId>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct StartAnimation {
    pub pool: Vec<ParticipantId>,
    pub result: ParticipantId,
}
