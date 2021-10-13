use super::{AssocParticipantInOtherRoom, ParticipantInOtherRoom};
use crate::db::rooms::RoomId;
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

/// Returns the name of the RabbitMQ topic exchange used to communicate across
/// parent/breakout rooms
///
/// Note that this exchange is used to communicate across breakout-room boundaries and
/// should only be used in special circumstances where that behavior is intended.
pub fn global_exchange_name(room: RoomId) -> String {
    format!("k3k-signaling.globalroom={}", room.into_inner())
}
