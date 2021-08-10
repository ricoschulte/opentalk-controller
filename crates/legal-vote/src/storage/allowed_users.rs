use anyhow::{Context, Result};
use controller::db::legal_votes::VoteId;
use controller::db::rooms::RoomId;
use controller::db::users::UserId;
use controller::prelude::*;
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room_id}:vote={vote_id}:allowed_users
#[ignore_extra_doc_attributes]
///
/// A set of users that are allowed to vote.
///
/// When a vote is casted, the requesting user has to be removed from this set in order to proceed.
/// When a user is not contained in this set, the user either voted already or was never allowed to vote.
///
/// See [`VOTE_SCRIPT`](super::VOTE_SCRIPT) for more details on the vote process.
pub(super) struct AllowedUsersKey {
    pub(super) room_id: RoomId,
    pub(super) vote_id: VoteId,
}

impl_to_redis_args!(AllowedUsersKey);

/// Set the list of allowed users for the provided `vote_id`
#[tracing::instrument(name = "legalvote_set_allowed_users", skip(redis_conn, allowed_users))]
pub(crate) async fn set(
    redis_conn: &mut ConnectionManager,
    room_id: RoomId,
    vote_id: VoteId,
    allowed_users: Vec<UserId>,
) -> Result<()> {
    redis_conn
        .sadd(AllowedUsersKey { room_id, vote_id }, allowed_users)
        .await
        .with_context(|| {
            format!(
                "Failed to set the allowed users for room_id:{} vote_id:{}",
                room_id, vote_id
            )
        })
}
