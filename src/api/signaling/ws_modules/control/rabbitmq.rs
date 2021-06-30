use crate::api::signaling::ParticipantId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Control messages sent between controller modules to communicate changes inside a room
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Message {
    Joined(ParticipantId),
    Left(ParticipantId),
    Update(ParticipantId),
}

/// Returns the name of the RabbitMQ topic exchange used to send messages between participants for
/// the given room-id
pub fn room_exchange_name(room: Uuid) -> String {
    format!("k3k-signaling.room.{}", room)
}

/// Returns the routing-key/topic used to send a message to the given participant
pub fn room_participant_routing_key(id: ParticipantId) -> String {
    format!("participant.{}", id)
}

/// Returns the routing-key/topic used to send a message to ALL participants inside a room
pub fn room_all_routing_key() -> &'static str {
    "participant.all"
}
