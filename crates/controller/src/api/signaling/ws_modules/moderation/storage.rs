use crate::redis_wrapper::RedisConnection;
use anyhow::{Context, Result};
use controller_shared::ParticipantId;
use db_storage::{rooms::RoomId, users::UserId};
use displaydoc::Display;
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
    redis_conn: &mut RedisConnection,
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
    redis_conn: &mut RedisConnection,
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
    redis_conn: &mut RedisConnection,
    room: RoomId,
    user_id: UserId,
) -> Result<bool> {
    redis_conn
        .sismember(Bans { room }, user_id)
        .await
        .context("Failed to SISMEMBER user_id on bans")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn delete_bans(redis_conn: &mut RedisConnection, room: RoomId) -> Result<()> {
    redis_conn
        .del(Bans { room })
        .await
        .context("Failed to DEL bans")
}

#[derive(Display)]
/// k3k-signaling:room={room}:waiting_room_enabled
#[ignore_extra_doc_attributes]
/// If set to true the waiting room is enabled
struct WaitingRoomEnabled {
    room: RoomId,
}

impl_to_redis_args!(WaitingRoomEnabled);

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn set_waiting_room_enabled(
    redis_conn: &mut RedisConnection,
    room: RoomId,
    enabled: bool,
) -> Result<()> {
    redis_conn
        .set(WaitingRoomEnabled { room }, enabled)
        .await
        .context("Failed to SET waiting_room_enabled")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn is_waiting_room_enabled(
    redis_conn: &mut RedisConnection,
    room: RoomId,
) -> Result<bool> {
    redis_conn
        .get(WaitingRoomEnabled { room })
        .await
        .context("Failed to GET waiting_room_enabled")
        .map(Option::<bool>::unwrap_or_default)
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn delete_waiting_room_enabled(
    redis_conn: &mut RedisConnection,
    room: RoomId,
) -> Result<()> {
    redis_conn
        .del(WaitingRoomEnabled { room })
        .await
        .context("Failed to DEL waiting_room_enabled")
}

#[derive(Display)]
/// k3k-signaling:room={room}:raise_hands_enabled
#[ignore_extra_doc_attributes]
/// If set to true the raise hands is enabled
struct RaiseHandsEnabled {
    room: RoomId,
}

impl_to_redis_args!(RaiseHandsEnabled);

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn set_raise_hands_enabled(
    redis_conn: &mut RedisConnection,
    room: RoomId,
    enabled: bool,
) -> Result<()> {
    redis_conn
        .set(RaiseHandsEnabled { room }, enabled)
        .await
        .context("Failed to SET raise_hands_enabled")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn is_raise_hands_enabled(
    redis_conn: &mut RedisConnection,
    room: RoomId,
) -> Result<bool> {
    redis_conn
        .get(RaiseHandsEnabled { room })
        .await
        .context("Failed to GET raise_hands_enabled")
        .map(|result: Option<bool>| result.unwrap_or(true))
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn delete_raise_hands_enabled(
    redis_conn: &mut RedisConnection,
    room: RoomId,
) -> Result<()> {
    redis_conn
        .del(RaiseHandsEnabled { room })
        .await
        .context("Failed to DEL raise_hands_enabled")
}

#[derive(Display)]
/// k3k-signaling:room={room}:waiting_room_list
#[ignore_extra_doc_attributes]
/// Set of participant ids inside the waiting room
struct WaitingRoomList {
    room: RoomId,
}

impl_to_redis_args!(WaitingRoomList);

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn waiting_room_add(
    redis_conn: &mut RedisConnection,
    room: RoomId,
    participant_id: ParticipantId,
) -> Result<usize> {
    redis_conn
        .sadd(WaitingRoomList { room }, participant_id)
        .await
        .context("Failed to SADD waiting_room_list")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn waiting_room_remove(
    redis_conn: &mut RedisConnection,
    room: RoomId,
    participant_id: ParticipantId,
) -> Result<()> {
    redis_conn
        .srem(WaitingRoomList { room }, participant_id)
        .await
        .context("Failed to SREM waiting_room_list")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn waiting_room_contains(
    redis_conn: &mut RedisConnection,
    room: RoomId,
    participant_id: ParticipantId,
) -> Result<bool> {
    redis_conn
        .srem(WaitingRoomList { room }, participant_id)
        .await
        .context("Failed to SREM waiting_room_list")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn waiting_room_all(
    redis_conn: &mut RedisConnection,
    room: RoomId,
) -> Result<Vec<ParticipantId>> {
    redis_conn
        .smembers(WaitingRoomList { room })
        .await
        .context("Failed to SMEMBERS waiting_room_list")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn waiting_room_len(redis_conn: &mut RedisConnection, room: RoomId) -> Result<usize> {
    redis_conn
        .scard(WaitingRoomList { room })
        .await
        .context("Failed to SCARD waiting_room_list")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn delete_waiting_room(redis_conn: &mut RedisConnection, room: RoomId) -> Result<()> {
    redis_conn
        .del(WaitingRoomList { room })
        .await
        .context("Failed to DEL waiting_room_list")
}
