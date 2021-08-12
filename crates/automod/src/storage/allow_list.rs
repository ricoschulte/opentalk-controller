//! List of participants used by selection_strategies to help decide who to select.
//! Empty allow_list will usually mean that every participant can be selected.
//!
//! Depending on the selection strategy:
//!
//! - `none`, `random` or `nomination`: The allow_list acts as pool of participants which can
//!   be selected (by nomination or randomly etc).
//!
//! - `playlist` The allow_list does not get used by this strategy.
// TODO: Playlist mode will use this to filter which participants can add themself to the playlist via hand-raise

use anyhow::{Context, Result};
use controller::db::rooms::RoomId;
use controller::prelude::*;
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room}:automod:allow_list
#[ignore_extra_doc_attributes]
/// Typed key to the allow_list
struct RoomAutoModAllowList {
    room: RoomId,
}

impl_to_redis_args!(RoomAutoModAllowList);

/// Override the current allow_list with the given one. If the `allow_list` parameter is empty,
/// the entry will just be deleted.
#[tracing::instrument(name = "set_allow_list", skip(redis_conn, allow_list))]
pub async fn set(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
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
    room: RoomId,
    participant: ParticipantId,
) -> Result<()> {
    redis_conn
        .srem(RoomAutoModAllowList { room }, participant)
        .await
        .context("Failed to remove participant from allow_list")
}

/// Get a random `participant` from the allow_list. Will return `None` if the allow_list if empty.
#[tracing::instrument(name = "random_member_allow_list", skip(redis_conn))]
pub async fn random(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
) -> Result<Option<ParticipantId>> {
    redis_conn
        .srandmember(RoomAutoModAllowList { room })
        .await
        .context("Failed to get random member from allow list")
}

/// Check if the given `participant` is allowed by the `allow_list`. An empty `allow_list` will
/// always return `true`.
#[tracing::instrument(skip(redis_conn))]
pub async fn is_allowed(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    participant: ParticipantId,
) -> Result<bool> {
    let exists: bool = redis_conn.exists(RoomAutoModAllowList { room }).await?;

    // empty allow list allows anyone
    if exists {
        redis_conn
            .sismember(RoomAutoModAllowList { room }, participant)
            .await
            .context("Failed to check if participant is inside allow_list")
    } else {
        Ok(true)
    }
}

/// Return all members of the `allow_list`.
#[tracing::instrument(name = "get_all_allow_list", skip(redis_conn))]
pub async fn get_all(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
) -> Result<Vec<ParticipantId>> {
    redis_conn
        .smembers(RoomAutoModAllowList { room })
        .await
        .context("Failed to get random member from allow list")
}

/// Delete the `allow_list`.
#[tracing::instrument(name = "del_allow_list", skip(redis_conn))]
pub async fn del(redis_conn: &mut ConnectionManager, room: RoomId) -> Result<()> {
    redis_conn
        .del(RoomAutoModAllowList { room })
        .await
        .context("Failed to del allow list")
}
