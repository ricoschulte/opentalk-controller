//! The configuration of the automod. If it exists inside of redis for a room, the room is
//! considered to being auto-moderated.

use crate::config::StorageConfig;
use anyhow::{Context, Result};
use controller::prelude::*;
use redis::AsyncCommands;
use redis_args::ToRedisArgs;

/// Typed key to the automod config
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:automod:config")]
pub struct RoomAutoModConfig {
    room: SignalingRoomId,
}

/// Set the current config.
#[tracing::instrument(name = "set_config", level = "debug", skip(redis_conn, config))]
pub async fn set(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    config: &StorageConfig,
) -> Result<()> {
    redis_conn
        .set(RoomAutoModConfig { room }, config)
        .await
        .context("Failed to set config")
}

/// Get the current config, if any is set.
///
/// If it returns `Some`, one must assume the automod is active.
#[tracing::instrument(name = "get_config", level = "debug", skip(redis_conn))]
pub async fn get(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
) -> Result<Option<StorageConfig>> {
    redis_conn
        .get(RoomAutoModConfig { room })
        .await
        .context("Failed to get config")
}

/// Delete the config.
#[tracing::instrument(name = "del_config", level = "debug", skip(redis_conn))]
pub async fn del(redis_conn: &mut RedisConnection, room: SignalingRoomId) -> Result<()> {
    redis_conn
        .del(RoomAutoModConfig { room })
        .await
        .context("Failed to del config")
}
