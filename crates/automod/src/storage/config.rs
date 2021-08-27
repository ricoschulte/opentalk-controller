//! The configuration of the automod. If it exists inside of redis for a room, the room is
//! considered to being auto-moderated.

use crate::config::StorageConfig;
use anyhow::{Context, Result};
use controller::db::rooms::RoomId;
use controller::prelude::*;
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room}:automod:config
#[ignore_extra_doc_attributes]
/// Typed key to the automod config
pub struct RoomAutoModConfig {
    room: RoomId,
}

impl_to_redis_args!(RoomAutoModConfig);
impl_to_redis_args_se!(&StorageConfig);
impl_from_redis_value_de!(StorageConfig);

/// Set the current config.
pub async fn set(
    redis: &mut ConnectionManager,
    room: RoomId,
    config: &StorageConfig,
) -> Result<()> {
    redis
        .set(RoomAutoModConfig { room }, config)
        .await
        .context("Failed to set config")
}

/// Get the current config, if any is set.
///
/// If it returns `Some`, one must assume the automod is active.
pub async fn get(redis: &mut ConnectionManager, room: RoomId) -> Result<Option<StorageConfig>> {
    redis
        .get(RoomAutoModConfig { room })
        .await
        .context("Failed to get config")
}

/// Delete the config.
pub async fn del(redis: &mut ConnectionManager, room: RoomId) -> Result<()> {
    redis
        .del(RoomAutoModConfig { room })
        .await
        .context("Failed to del config")
}
