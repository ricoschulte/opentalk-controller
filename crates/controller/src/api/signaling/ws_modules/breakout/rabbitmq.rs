use super::{AssocParticipantInOtherRoom, ParticipantInOtherRoom};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Message {
    Start(Start),
    Stop,

    Joined(ParticipantInOtherRoom),
    Left(AssocParticipantInOtherRoom),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Start {
    pub ws_start: super::incoming::Start,
    pub started: SystemTime,
}
