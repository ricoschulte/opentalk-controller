use anyhow::{Context, Result};
use controller::prelude::*;
use db_storage::legal_votes::types::{Tally, VoteOption};
use db_storage::legal_votes::LegalVoteId;
use redis::AsyncCommands;
use redis_args::ToRedisArgs;
use std::collections::HashMap;

/// Contains a sorted set of [`VoteOption`] each with their respective vote count.
///
/// When a vote is casted, the corresponding vote option in this list will get incremented.
/// See [`VOTE_SCRIPT`](super::VOTE_SCRIPT) for more details on the vote process.
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room_id}:vote={legal_vote_id}:vote_count")]
pub(super) struct VoteCountKey {
    pub(super) room_id: SignalingRoomId,
    pub(super) legal_vote_id: LegalVoteId,
}

/// Get the vote count for the specified `legal_vote_id`
#[tracing::instrument(name = "legal_vote_get_vote_count", skip(redis_conn))]
pub(crate) async fn get(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
    legal_vote_id: LegalVoteId,
    enable_abstain: bool,
) -> Result<Tally> {
    let vote_count: HashMap<VoteOption, u64> = redis_conn
        .zrange_withscores(
            VoteCountKey {
                room_id,
                legal_vote_id,
            },
            0,
            -1,
        )
        .await
        .with_context(|| {
            format!(
                "Failed to get the vote count for room_id:{} legal_vote_id:{}",
                room_id, legal_vote_id
            )
        })?;

    Ok(Tally {
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
