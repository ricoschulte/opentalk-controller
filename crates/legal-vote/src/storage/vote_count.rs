use anyhow::{Context, Result};
use controller::db::legal_votes::VoteId;
use controller::prelude::*;
use db_storage::legal_votes::types::{VoteOption, Votes};
use displaydoc::Display;
use redis::{aio::ConnectionManager, AsyncCommands};
use std::collections::HashMap;

#[derive(Display)]
/// k3k-signaling:room={room_id}:vote={vote_id}:vote_count
#[ignore_extra_doc_attributes]
///
/// Contains a sorted set of [`VoteOption`] each with their respective vote count.
///
/// When a vote is casted, the corresponding vote option in this list will get incremented.
/// See [`VOTE_SCRIPT`](super::VOTE_SCRIPT) for more details on the vote process.
pub(super) struct VoteCountKey {
    pub(super) room_id: SignalingRoomId,
    pub(super) vote_id: VoteId,
}

impl_to_redis_args!(VoteCountKey);

/// Get the vote count for the specified `vote_id`
#[tracing::instrument(name = "legalvote_get_vote_count", skip(redis_conn))]
pub(crate) async fn get(
    redis_conn: &mut ConnectionManager,
    room_id: SignalingRoomId,
    vote_id: VoteId,
    enable_abstain: bool,
) -> Result<Votes> {
    let vote_count: HashMap<VoteOption, u64> = redis_conn
        .zrange_withscores(VoteCountKey { room_id, vote_id }, 0, -1)
        .await
        .with_context(|| {
            format!(
                "Failed to get the vote count for room_id:{} vote_id:{}",
                room_id, vote_id
            )
        })?;

    Ok(Votes {
        yes: *vote_count.get(&VoteOption::Yes).unwrap_or(&0),
        no: *vote_count.get(&VoteOption::No).unwrap_or(&0),
        abstain: {
            if enable_abstain {
                Some(*vote_count.get(&VoteOption::Abstain).unwrap_or(&0))
            } else {
                None
            }
        },
    })
}
