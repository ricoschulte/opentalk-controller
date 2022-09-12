use crate::api::signaling::SignalingRoomId;
use crate::redis_wrapper::RedisConnection;
use anyhow::{Context, Result};
use displaydoc::Display;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

use super::RecordingId;

#[derive(Display)]
/// k3k-signaling:room={room_id}:recording:init
#[ignore_extra_doc_attributes]
///
/// Stores the [`RecordingState`] of this room.
struct RecordingStateKey {
    room_id: SignalingRoomId,
}

impl_to_redis_args!(RecordingStateKey);

/// State of the recording
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "recording_id", rename_all = "snake_case")]
pub enum RecordingState {
    /// Waiting for a recorder to connect and start the recording
    Initializing,
    /// A recorder is connected and capturing the conference
    Recording(RecordingId),
}

impl_to_redis_args_se!(RecordingState);
impl_from_redis_value_de!(RecordingState);

pub(super) async fn try_init(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
) -> Result<bool> {
    redis_conn
        .set_nx(RecordingStateKey { room_id }, RecordingState::Initializing)
        .await
        .context("Failed to initialize recording state")
}

pub(super) async fn set_recording(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
    id: RecordingId,
) -> Result<()> {
    redis_conn
        .set(RecordingStateKey { room_id }, RecordingState::Recording(id))
        .await
        .context("Failed to set recording state to 'recording'")
}

pub(super) async fn get_state(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
) -> Result<Option<RecordingState>> {
    redis_conn
        .get(RecordingStateKey { room_id })
        .await
        .context("Failed to get recording state")
}

pub(super) async fn del_state(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
) -> Result<()> {
    redis_conn
        .del(RecordingStateKey { room_id })
        .await
        .context("Failed to delete recording state")
}
