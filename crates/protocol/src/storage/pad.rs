use anyhow::{Context, Result};
use controller::prelude::*;
use displaydoc::Display;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room_id}:protocol:readonly_pad
#[ignore_extra_doc_attributes]
///
/// Stores the readonly pad-id for the specified room
pub(super) struct ReadonlyPadKey {
    pub(super) room_id: SignalingRoomId,
}

impl_to_redis_args!(ReadonlyPadKey);

#[tracing::instrument(name = "set_protocol_readonly_pad", skip(redis_conn))]
pub(crate) async fn set_readonly(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
    readonly_id: &str,
) -> Result<()> {
    redis_conn
        .set(ReadonlyPadKey { room_id }, readonly_id)
        .await
        .context("Failed to set readonly protocol pad key")
}

#[tracing::instrument(name = "get_protocol_readonly_pad", skip(redis_conn))]
pub(crate) async fn get_readonly(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
) -> Result<String> {
    redis_conn
        .get(ReadonlyPadKey { room_id })
        .await
        .context("Failed to get readonly protocol pad key")
}

#[tracing::instrument(name = "delete_protocol_readonly_pad", skip(redis_conn))]
pub(crate) async fn del_readonly(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
) -> Result<()> {
    redis_conn
        .del(ReadonlyPadKey { room_id })
        .await
        .context("Failed to delete readonly protocol pad key")
}
