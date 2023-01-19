// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use anyhow::{Context, Result};
use controller::prelude::*;
use redis::AsyncCommands;
use redis_args::ToRedisArgs;

/// Stores the etherpad group_id that is associated with this room.
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room_id}:protocol:group")]
pub(super) struct GroupKey {
    pub(super) room_id: SignalingRoomId,
}

#[tracing::instrument(name = "set_protocol_group", skip(redis_conn))]
pub(crate) async fn set(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
    group_id: &str,
) -> Result<()> {
    redis_conn
        .set(GroupKey { room_id }, group_id)
        .await
        .context("Failed to set protocol group key")
}

#[tracing::instrument(name = "get_protocol_group", skip(redis_conn))]
pub(crate) async fn get(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
) -> Result<Option<String>> {
    redis_conn
        .get(GroupKey { room_id })
        .await
        .context("Failed to get protocol group key")
}

#[tracing::instrument(name = "delete_protocol_group", skip(redis_conn))]
pub(crate) async fn del(redis_conn: &mut RedisConnection, room_id: SignalingRoomId) -> Result<()> {
    redis_conn
        .del(GroupKey { room_id })
        .await
        .context("Failed to delete protocol group key")
}
