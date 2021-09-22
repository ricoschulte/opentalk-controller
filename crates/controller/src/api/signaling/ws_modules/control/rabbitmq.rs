use crate::api::signaling::{ParticipantId, SignalingRoomId};
use crate::db::rooms::RoomId;
use serde::{Deserialize, Serialize};

/// Control messages sent between controller modules to communicate changes inside a room
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Message {
    Joined(ParticipantId),
    Left(ParticipantId),
    Update(ParticipantId),
}

/// Returns the name of the RabbitMQ topic exchange used to communicate inside a room.
///
/// Note that this exchange is used to communicate across breakout-room boundaries and
/// should only be used in special circumstances where that behavior is intended.
pub fn room_exchange_name(room: RoomId) -> String {
    format!("k3k-signaling.room={}", room.into_inner())
}

/// Returns the name of the RabbitMQ topic exchange used inside the current room.
/// This exchange should be used when writing behavior constrained to a single room
pub fn current_room_exchange_name(room: SignalingRoomId) -> String {
    format!("k3k-signaling.room={}", room)
}

/// Returns the routing-key/topic used to send a message to the given participant
pub fn room_participant_routing_key(id: ParticipantId) -> String {
    format!("participant.{}", id)
}

/// Returns the routing-key/topic used to send a message to ALL participants inside a room
pub fn room_all_routing_key() -> &'static str {
    "participant.all"
}
