use anyhow::{Context, Result};
use controller::prelude::*;
use db_storage::legal_votes::types::Parameters;
use db_storage::legal_votes::LegalVoteId;
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room_id}:vote={legal_vote_id}
#[ignore_extra_doc_attributes]
///
/// Contains the [`Parameters`] of the a vote.
pub(super) struct VoteParametersKey {
    pub(super) room_id: SignalingRoomId,
    pub(super) legal_vote_id: LegalVoteId,
}

impl_to_redis_args!(VoteParametersKey);

/// Set the vote [`Parameters`] for the provided `legal_vote_id`
#[tracing::instrument(name = "legal_vote_set_parameters", skip(redis_conn, parameters))]
pub(crate) async fn set(
    redis_conn: &mut ConnectionManager,
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
    redis_conn: &mut ConnectionManager,
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
