use anyhow::Result;
use controller::prelude::*;

pub(crate) mod group;
pub(crate) mod init;
pub(crate) mod pad;
pub(crate) mod session;

/// Remove all redis keys related to this room & module
#[tracing::instrument(name = "cleanup_protocol", skip(redis_conn))]
pub(crate) async fn cleanup(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
) -> Result<()> {
    init::del(redis_conn, room_id).await?;
    group::del(redis_conn, room_id).await?;
    pad::del_readonly(redis_conn, room_id).await?;

    Ok(())
}
