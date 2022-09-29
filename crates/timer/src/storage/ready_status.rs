use anyhow::{Context, Result};
use controller::prelude::*;
use controller_shared::ParticipantId;
use displaydoc::Display;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

#[derive(Display)]
/// k3k-signaling:room={room_id}:participant::{participant_id}::timer-ready-status
#[ignore_extra_doc_attributes]
/// A key to track the participants ready status
struct ReadyStatusKey {
    room_id: SignalingRoomId,
    participant_id: ParticipantId,
}

impl_to_redis_args!(ReadyStatusKey);

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyStatus {
    pub ready_status: bool,
}

impl_to_redis_args_se!(&ReadyStatus);
impl_from_redis_value_de!(ReadyStatus);

/// Set the ready status of a participant
#[tracing::instrument(name = "meeting_timer_ready_set", skip(redis_conn))]
pub(crate) async fn set(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
    participant_id: ParticipantId,
    ready_status: bool,
) -> Result<()> {
    redis_conn
        .set(
            ReadyStatusKey {
                room_id,
                participant_id,
            },
            &ReadyStatus { ready_status },
        )
        .await
        .context("Failed to set ready state")
}

/// Get the ready status of a participant
#[tracing::instrument(name = "meeting_timer_ready_get", skip(redis_conn))]
pub(crate) async fn get(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
    participant_id: ParticipantId,
) -> Result<Option<ReadyStatus>> {
    redis_conn
        .get(ReadyStatusKey {
            room_id,
            participant_id,
        })
        .await
        .context("Failed to get ready state")
}

/// Delete the ready status of a participant
#[tracing::instrument(name = "meeting_timer_ready_delete", skip(redis_conn))]
pub(crate) async fn delete(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
    participant_id: ParticipantId,
) -> Result<()> {
    redis_conn
        .del(ReadyStatusKey {
            room_id,
            participant_id,
        })
        .await
        .context("Failed to delete ready state")
}
