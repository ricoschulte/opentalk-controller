use anyhow::{Context, Result};
use controller::prelude::*;
use displaydoc::Display;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room_id}:protocol:group
#[ignore_extra_doc_attributes]
///
/// Stores the etherpad group_id that is associated with this room.
pub(super) struct GroupKey {
    pub(super) room_id: SignalingRoomId,
}

impl_to_redis_args!(GroupKey);

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
