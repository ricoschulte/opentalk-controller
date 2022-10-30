// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::api::signaling::SignalingRoomId;
use crate::redis_wrapper::RedisConnection;
use anyhow::{Context, Result};
use redis::AsyncCommands;
use redis_args::{FromRedisValue, ToRedisArgs};
use serde::{Deserialize, Serialize};

use super::RecordingId;

/// Stores the [`RecordingState`] of this room.
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room_id}:recording:init")]
struct RecordingStateKey {
    room_id: SignalingRoomId,
}

/// State of the recording
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, ToRedisArgs, FromRedisValue)]
#[serde(tag = "state", content = "recording_id", rename_all = "snake_case")]
#[to_redis_args(serde)]
#[from_redis_value(serde)]
pub enum RecordingState {
    /// Waiting for a recorder to connect and start the recording
    Initializing,
    /// A recorder is connected and capturing the conference
    Recording(RecordingId),
}

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
