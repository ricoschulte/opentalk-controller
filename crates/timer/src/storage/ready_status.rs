use anyhow::{Context, Result};
use controller::prelude::*;
use controller_shared::ParticipantId;
use redis::AsyncCommands;
use redis_args::{FromRedisValue, ToRedisArgs};
use serde::{Deserialize, Serialize};

/// A key to track the participants ready status
#[derive(ToRedisArgs)]
#[to_redis_args(
    fmt = "k3k-signaling:room={room_id}:participant::{participant_id}::timer-ready-status"
)]
struct ReadyStatusKey {
    room_id: SignalingRoomId,
    participant_id: ParticipantId,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, ToRedisArgs, FromRedisValue)]
#[to_redis_args(serde)]
#[from_redis_value(serde)]
pub struct ReadyStatus {
    pub ready_status: bool,
}

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
