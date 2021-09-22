use anyhow::{Context, Result};
use controller::db::legal_votes::VoteId;
use controller::prelude::*;
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room_id}:vote:current
#[ignore_extra_doc_attributes]
///
/// Contains the [`VoteId`] of the active vote.
///
/// The current vote id key acts like a kind of lock. When a vote is in progress and therefore
/// this key has a value, no new vote can be started. This key gets deleted when a vote ends.
///
/// See [`END_CURRENT_VOTE_SCRIPT`](super::END_CURRENT_VOTE_SCRIPT) for more details.
pub(super) struct CurrentVoteIdKey {
    pub(super) room_id: SignalingRoomId,
}

impl_to_redis_args!(CurrentVoteIdKey);

/// Set the current vote id to `new_vote_id`
///
/// Set the current vote id only if the key does not exist yet.
///
/// # Returns
/// - `Ok(true)` when the key got set.
/// - `Ok(false)` when the key already exists and no changes were made.
/// - `Err(anyhow::Error)` when a redis error occurred.
#[tracing::instrument(name = "legalvote_set_current_vote_id", skip(redis_conn))]
pub(crate) async fn set(
    redis_conn: &mut ConnectionManager,
    room_id: SignalingRoomId,
    new_vote_id: VoteId,
) -> Result<bool> {
    // set if not exists
    let affected_entries: i64 = redis_conn
        .set_nx(CurrentVoteIdKey { room_id }, new_vote_id)
        .await
        .context("Failed to set current vote id")?;

    if affected_entries == 1 {
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Get the currently active vote id
#[tracing::instrument(name = "legalvote_get_current_vote_id", skip(redis_conn))]
pub(crate) async fn get(
    redis_conn: &mut ConnectionManager,
    room_id: SignalingRoomId,
) -> Result<Option<VoteId>> {
    redis_conn
        .get(CurrentVoteIdKey { room_id })
        .await
        .context("Failed to get current vote id")
}

/// Delete the current vote id key
#[tracing::instrument(name = "legalvote_delete_current_vote_id", skip(redis_conn))]
pub(crate) async fn delete(
    redis_conn: &mut ConnectionManager,
    room_id: SignalingRoomId,
) -> Result<()> {
    redis_conn
        .del(CurrentVoteIdKey { room_id })
        .await
        .context("Failed to delete current vote id key")
}
