use crate::api::signaling::SignalingRoomId;
use controller_shared::ParticipantId;
use db_storage::users::UserId;
use serde::{Deserialize, Serialize};

/// Control messages sent between controller modules to communicate changes inside a room
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Message {
    /// Participant with the given id joined the current room
    Joined(ParticipantId),

    /// Participant with the given id left the current room
    Left(ParticipantId),

    /// Participant with the given id updated its status
    Update(ParticipantId),

    /// Participant with the given id, was accepted into the room
    ///
    /// This message is only sent to the participant to be accepted and published by the `moderation` module.
    /// The control module only handles the joining into the waiting room and joining the actual room.
    Accepted(ParticipantId),

    SetModeratorStatus(bool),
}

/// Returns the name of the RabbitMQ topic exchange used inside the current room.
/// This exchange should be used when writing behavior constrained to a single room
pub fn current_room_exchange_name(room: SignalingRoomId) -> String {
    format!("k3k-signaling.room={}", room)
}

/// Returns the routing-key/topic used to send a message to the given user
pub fn room_user_routing_key(id: UserId) -> String {
    format!("user.{}", id)
}

/// Returns the routing-key/topic used to send a message to the given participant
pub fn room_participant_routing_key(id: ParticipantId) -> String {
    format!("participant.{}", id)
}

/// Returns the routing-key/topic used to send a message to ALL participants inside a room
pub fn room_all_routing_key() -> &'static str {
    "participant.all"
}
