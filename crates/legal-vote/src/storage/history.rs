use anyhow::{Context, Result};
use controller::prelude::*;
use db_storage::legal_votes::LegalVoteId;
use displaydoc::Display;
use redis::AsyncCommands;
use std::collections::HashSet;

#[derive(Display)]
/// k3k-signaling:room={room_id}:vote:history
#[ignore_extra_doc_attributes]
///
/// Contains a set of [`VoteId`] from all votes that were completed since the start of this room.
///
/// When a vote is stopped or canceled, the vote id will be added to this key.
/// See [`END_CURRENT_VOTE_SCRIPT`](super::END_CURRENT_VOTE_SCRIPT) for more details.
pub(super) struct VoteHistoryKey {
    pub(super) room_id: SignalingRoomId,
}

impl_to_redis_args!(VoteHistoryKey);

/// Get the vote history as a hashset
#[tracing::instrument(name = "legal_vote_get_history", skip(redis_conn))]
pub(crate) async fn get(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
) -> Result<HashSet<LegalVoteId>> {
    redis_conn
        .smembers(VoteHistoryKey { room_id })
        .await
        .context("Failed to get vote history")
}

/// Delete the vote history key
#[tracing::instrument(name = "legal_vote_delete_history", skip(redis_conn))]
pub(crate) async fn delete(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
) -> Result<()> {
    redis_conn
        .del(VoteHistoryKey { room_id })
        .await
        .context("Failed to remove vote history")
}
