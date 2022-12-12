use anyhow::{Context, Result};
use controller::prelude::*;
use db_storage::legal_votes::types::Token;
use db_storage::legal_votes::LegalVoteId;
use redis::AsyncCommands;
use redis_args::ToRedisArgs;

/// A set of tokens that can be used to vote.
///
/// When a vote is casted, the consumed token will be removed from this set in order to proceed.
/// When a token is not contained in this set, the token is either consumed already or was never allowed to be used for voting.
///
/// See [`VOTE_SCRIPT`](super::VOTE_SCRIPT) for more details on the vote process.
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room_id}:vote={legal_vote_id}:allowed_tokens")]
pub(super) struct AllowedTokensKey {
    pub(super) room_id: SignalingRoomId,
    pub(super) legal_vote_id: LegalVoteId,
}

/// Set the list of allowed tokens for the provided `legal_vote_id`
#[tracing::instrument(
    name = "legal_vote_set_allowed_tokens",
    skip(redis_conn, allowed_tokens)
)]
pub(crate) async fn set(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
    legal_vote_id: LegalVoteId,
    allowed_tokens: Vec<Token>,
) -> Result<()> {
    redis_conn
        .sadd(
            AllowedTokensKey {
                room_id,
                legal_vote_id,
            },
            allowed_tokens,
        )
        .await
        .with_context(|| {
            format!(
                "Failed to set the allowed tokens for room_id:{} legal_vote_id:{}",
                room_id, legal_vote_id
            )
        })
}
