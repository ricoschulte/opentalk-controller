//! List of participants used by selection_strategies to help decide who to select.
//!
//! Depending on the selection strategy:
//!
//! - `none`, `random` or `nomination`: The allow_list acts as pool of participants which can
//!   be selected (by nomination or randomly etc).
//!
//! - `playlist` The allow_list does not get used by this strategy.
// TODO: Playlist mode will use this to filter which participants can add themself to the playlist via hand-raise

use anyhow::{Context, Result};
use controller::prelude::*;
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room}:automod:allow_list
#[ignore_extra_doc_attributes]
/// Typed key to the allow_list
struct RoomAutoModAllowList {
    room: SignalingRoomId,
}

impl_to_redis_args!(RoomAutoModAllowList);

/// Override the current allow_list with the given one. If the `allow_list` parameter is empty,
/// the entry will just be deleted.
#[tracing::instrument(name = "set_allow_list", skip(redis_conn, allow_list))]
pub async fn set(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
    allow_list: &[ParticipantId],
) -> Result<()> {
    redis_conn
        .del(RoomAutoModAllowList { room })
        .await
        .context("Failed to delete playlist to later reinsert it")?;

    if !allow_list.is_empty() {
        redis_conn
            .sadd(RoomAutoModAllowList { room }, allow_list)
            .await
            .context("Failed to insert new allow_list set")
    } else {
        Ok(())
    }
}

/// Remove the given participant from the allow_list
#[tracing::instrument(name = "remove_from_allow_list", skip(redis_conn))]
pub async fn remove(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<usize> {
    redis_conn
        .srem(RoomAutoModAllowList { room }, participant)
        .await
        .context("Failed to remove participant from allow_list")
}

/// Get a random `participant` from the allow_list. Will return `None` if the allow_list if empty.
#[tracing::instrument(name = "random_member_allow_list", skip(redis_conn))]
pub async fn random(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
) -> Result<Option<ParticipantId>> {
    redis_conn
        .srandmember(RoomAutoModAllowList { room })
        .await
        .context("Failed to get random member from allow list")
}

/// Pop a random `participant` from the allow_list. Will return `None` if the allow_list if empty.
#[tracing::instrument(skip(redis_conn))]
pub async fn pop_random(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
) -> Result<Option<ParticipantId>> {
    redis_conn
        .spop(RoomAutoModAllowList { room })
        .await
        .context("Failed to pop random member from allow list")
}

/// Check if the given `participant` is allowed by the `allow_list`.
#[tracing::instrument(skip(redis_conn))]
pub async fn is_allowed(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<bool> {
    redis_conn
        .sismember(RoomAutoModAllowList { room }, participant)
        .await
        .context("Failed to check if participant is inside allow_list")
}

/// Return all members of the `allow_list`.
#[tracing::instrument(name = "get_all_allow_list", skip(redis_conn))]
pub async fn get_all(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
) -> Result<Vec<ParticipantId>> {
    redis_conn
        .smembers(RoomAutoModAllowList { room })
        .await
        .context("Failed to get random member from allow list")
}

/// Delete the `allow_list`.
#[tracing::instrument(name = "del_allow_list", skip(redis_conn))]
pub async fn del(redis_conn: &mut ConnectionManager, room: SignalingRoomId) -> Result<()> {
    redis_conn
        .del(RoomAutoModAllowList { room })
        .await
        .context("Failed to del allow list")
}
