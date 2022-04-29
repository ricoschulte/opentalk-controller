use anyhow::{Context, Result};
use db_storage::{rooms::RoomId, users::UserId};
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room}:bans
#[ignore_extra_doc_attributes]
/// Set of user-ids banned in a room
struct Bans {
    room: RoomId,
}

impl_to_redis_args!(Bans);

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn ban_user(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    user_id: UserId,
) -> Result<()> {
    redis_conn
        .sadd(Bans { room }, user_id)
        .await
        .context("Failed to SADD user_id to bans")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn unban_user(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    user_id: UserId,
) -> Result<()> {
    redis_conn
        .srem(Bans { room }, user_id)
        .await
        .context("Failed to SREM user_id to bans")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn is_banned(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    user_id: UserId,
) -> Result<bool> {
    redis_conn
        .sismember(Bans { room }, user_id)
        .await
        .context("Failed to SISMEMBER user_id on bans")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn delete_bans(redis_conn: &mut ConnectionManager, room: RoomId) -> Result<()> {
    redis_conn
        .del(Bans { room })
        .await
        .context("Failed to DEL bans")
}
