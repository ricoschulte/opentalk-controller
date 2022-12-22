use anyhow::{Context, Result};
use controller::prelude::*;
use db_storage::legal_votes::types::Parameters;
use db_storage::legal_votes::LegalVoteId;
use redis::AsyncCommands;
use redis_args::ToRedisArgs;

/// Contains the [`Parameters`] of the a vote.
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room_id}:vote={legal_vote_id}")]
pub(super) struct VoteParametersKey {
    pub(super) room_id: SignalingRoomId,
    pub(super) legal_vote_id: LegalVoteId,
}

/// Set the vote [`Parameters`] for the provided `legal_vote_id`
#[tracing::instrument(name = "legal_vote_set_parameters", skip(redis_conn, parameters))]
pub(crate) async fn set(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
    legal_vote_id: LegalVoteId,
    parameters: &Parameters,
) -> Result<()> {
    redis_conn
        .set(
            VoteParametersKey {
                room_id,
                legal_vote_id,
            },
            parameters,
        )
        .await
        .with_context(|| {
            format!(
                "Failed to set the vote parameter for room_id:{} legal_vote_id:{}",
                room_id, legal_vote_id
            )
        })
}

/// Get the [`Parameters`] for the provided `legal_vote_id`
#[tracing::instrument(name = "legal_vote_get_parameters", skip(redis_conn))]
pub(crate) async fn get(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
    legal_vote_id: LegalVoteId,
) -> Result<Option<Parameters>> {
    redis_conn
        .get(VoteParametersKey {
            room_id,
            legal_vote_id,
        })
        .await
        .with_context(|| {
            format!(
                "Failed to get the vote parameter for room_id:{} legal_vote_id:{}",
                room_id, legal_vote_id
            )
        })
}
