use crate::api::signaling::ParticipantId;
use redis::{RedisWrite, ToRedisArgs};
use std::borrow::Cow;
use std::fmt;
use uuid::Uuid;

/// This enum represents all kinds of redis keys possible.
pub enum RedisKey<'s> {
    /// k3k-signaling:room={room-id}:namespace={namespace}
    ///
    /// A hashmap public insensitive data related to the room
    Room(Uuid, Cow<'s, str>),

    /// k3k-signaling:room={room-id}:participants
    ///
    /// A set of participant ids inside the room
    RoomParticipants(Uuid),

    /// k3k-signaling:room={room-id}:participant={participant-id}:namespace={namespace}
    ///
    /// A hashmap public insensitive data related to the participant
    RoomParticipant(Uuid, ParticipantId, Cow<'s, str>),
}

impl fmt::Display for RedisKey<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RedisKey::Room(room, namespace) => {
                write!(f, "k3k-signaling:room={}:namespace={}", room, namespace)
            }
            RedisKey::RoomParticipants(room) => {
                write!(f, "k3k-signaling:room={}:participants", room)
            }
            RedisKey::RoomParticipant(room, participant, namespace) => {
                write!(
                    f,
                    "k3k-signaling:room={}:participant={}:namespace={}",
                    room, participant, namespace
                )
            }
        }
    }
}

impl ToRedisArgs for RedisKey<'_> {
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + RedisWrite,
    {
        out.write_arg_fmt(self)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const NIL_PARTICIPANTS_KEY: &str =
        "k3k-signaling:room=00000000-0000-0000-0000-000000000000:participants";
    const NIL_PUBLIC_PARTICIPANT: &str =
        "k3k-signaling:room=00000000-0000-0000-0000-000000000000:participant=00000000-0000-0000-0000-000000000000:namespace=control";
    const NIL_PUBLIC_ROOM: &str =
        "k3k-signaling:room=00000000-0000-0000-0000-000000000000:namespace=control";

    #[test]
    fn room_participants_display() {
        assert_eq!(
            RedisKey::RoomParticipants(Uuid::nil()).to_string(),
            NIL_PARTICIPANTS_KEY
        )
    }

    #[test]
    fn room_participant_public_display() {
        assert_eq!(
            RedisKey::RoomParticipant(Uuid::nil(), ParticipantId::nil(), Cow::Borrowed("control"))
                .to_string(),
            NIL_PUBLIC_PARTICIPANT
        )
    }

    #[test]
    fn room_public_display() {
        assert_eq!(
            RedisKey::Room(Uuid::nil(), Cow::Borrowed("control")).to_string(),
            NIL_PUBLIC_ROOM
        )
    }
}
