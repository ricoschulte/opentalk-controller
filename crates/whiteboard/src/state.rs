use anyhow::{Context, Result};
use controller::prelude::redis::AsyncCommands;
use controller::prelude::SignalingRoomId;
use controller::prelude::*;
use redis_args::{FromRedisValue, ToRedisArgs};
use serde::{Deserialize, Serialize};
use url::Url;

/// Stores the [`InitState`] of this room.
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room_id}:spacedeck:init")]
pub(super) struct InitStateKey {
    pub(super) room_id: SignalingRoomId,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, ToRedisArgs, FromRedisValue)]
#[to_redis_args(serde)]
#[from_redis_value(serde)]
pub enum InitState {
    /// Spacedeck is initializing
    Initializing,
    /// Spacedeck has been initialized
    Initialized(SpaceInfo),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceInfo {
    pub id: String,
    pub url: Url,
}

/// Attempts to set the spacedeck state to [`InitState::Initializing`] with a SETNX command.
///
/// If the key already holds a value, the current key gets returned without changing the state.
///
/// Behaves like a SETNX-GET redis command.
///
/// When the key was empty and the `Initializing` state was set, Ok(None) will be returned.
#[tracing::instrument(err, skip_all)]
#[tracing::instrument(name = "spacedeck_try_start_init", skip(redis_conn))]
pub(crate) async fn try_start_init(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
) -> Result<Option<InitState>> {
    let affected_entries: i64 = redis_conn
        .set_nx(InitStateKey { room_id }, InitState::Initializing)
        .await
        .context("Failed to set spacedeck init state")?;

    if affected_entries == 1 {
        Ok(None)
    } else {
        let state: InitState = redis_conn
            .get(InitStateKey { room_id })
            .await
            .context("Failed to get spacedeck init state")?;

        Ok(Some(state))
    }
}

/// Sets the room state to [`InitState::Initialized(..)`]
#[tracing::instrument(name = "spacedeck_set_initialized", skip(redis_conn, space_info))]
pub(crate) async fn set_initialized(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
    space_info: SpaceInfo,
) -> Result<()> {
    let initialized = InitState::Initialized(space_info);

    redis_conn
        .set(InitStateKey { room_id }, &initialized)
        .await
        .context("Failed to set spacedeck init state to `Initialized`")
}

#[tracing::instrument(name = "get_spacedeck_init_state", skip(redis_conn))]
pub(crate) async fn get(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
) -> Result<Option<InitState>> {
    redis_conn
        .get(InitStateKey { room_id })
        .await
        .context("Failed to get spacedeck init state")
}

#[tracing::instrument(name = "delete_spacedeck _init_state", skip(redis_conn))]
pub(crate) async fn del(redis_conn: &mut RedisConnection, room_id: SignalingRoomId) -> Result<()> {
    redis_conn
        .del::<_, i64>(InitStateKey { room_id })
        .await
        .context("Failed to delete spacedeck init key")?;

    Ok(())
}
