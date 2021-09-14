//! This module manages the vote related redis keys.
//!
//! Contains Lua scripts to manipulate multiple redis keys atomically in one request.
//!
//! Each key is defined in its own module with its related functions.
use self::protocol::{ProtocolEntry, VoteEvent};
use super::VoteOption;
use allowed_users::AllowedUsersKey;
use anyhow::{Context, Result};
use controller::db::legal_votes::VoteId;
use controller::db::rooms::RoomId;
use controller::db::users::UserId;
use controller::prelude::*;
use current_vote_id::CurrentVoteIdKey;
use history::VoteHistoryKey;
use parameters::VoteParametersKey;
use protocol::ProtocolKey;
use redis::aio::ConnectionManager;
use redis::FromRedisValue;
use vote_count::VoteCountKey;

pub(crate) mod allowed_users;
pub(crate) mod current_vote_id;
pub(crate) mod history;
pub(crate) mod parameters;
pub(crate) mod protocol;
pub(crate) mod vote_count;

/// Remove the current vote id and add it to the vote history.
/// Adds the provided protocol entry to the corresponding vote protocol.
///
/// The following parameters have to be provided:
///```text
/// KEYS[1] = current vote key
/// KEYS[2] = vote protocol key
/// KEYS[3] = vote history key
///
/// ARGV[1] = vote id
/// ARGV[2] = stop/cancel entry
///```
const END_CURRENT_VOTE_SCRIPT: &str = r#"
if (redis.call("get", KEYS[1]) == ARGV[1]) then
  redis.call("del", KEYS[1])
else
  return 0
end

redis.call("rpush", KEYS[2], ARGV[2])
redis.call("sadd", KEYS[3], ARGV[1])

return 1
"#;

/// End the current vote by moving the vote id to the history & adding a stop/cancel entry
/// to the vote protocol. See [`END_CURRENT_VOTE_SCRIPT`] for details.
///
/// #Returns
/// `Ok(true)` when the vote_id was successfully moved
/// `Ok(false)` when there is no current vote active
/// `Err(anyhow::Error)` when a redis error occurred
#[tracing::instrument(name = "legalvote_end_current_vote", skip(redis_conn, end_entry))]
pub(crate) async fn end_current_vote(
    redis_conn: &mut ConnectionManager,
    vote_id: VoteId,
    room_id: RoomId,
    end_entry: ProtocolEntry,
) -> Result<bool> {
    redis::Script::new(END_CURRENT_VOTE_SCRIPT)
        .key(CurrentVoteIdKey { room_id })
        .key(ProtocolKey { room_id, vote_id })
        .key(VoteHistoryKey { room_id })
        .arg(vote_id)
        .arg(end_entry)
        .invoke_async(redis_conn)
        .await
        .context("Failed to end current vote")
}

/// Remove all redis entries that are associated with a vote
///
/// The following parameters have to be provided:
/// ```text
/// KEYS[1] = current vote key
/// KEYS[2] = vote count key
/// KEYS[3] = vote parameters key
/// KEYS[4] = allowed users key
/// KEYS[5] = vote protocol key
///
/// ARGV[1] = vote_id
///
/// ```
const CLEANUP_SCRIPT: &str = r#"
if (redis.call("get", KEYS[1]) == ARGV[1]) then
  redis.call("del", KEYS[1])
end

redis.call("del", KEYS[2])
redis.call("del", KEYS[3])
redis.call("del", KEYS[4])
redis.call("del", KEYS[5])
"#;

/// Cleanup redis keys related to a vote
///
/// See [`CLEANUP_SCRIPT`] for details.
///
/// Deletes all entries associated with the room & vote id.
#[tracing::instrument(name = "legalvote_cleanup_vote", skip(redis_conn))]
pub(crate) async fn cleanup_vote(
    redis_conn: &mut ConnectionManager,
    room_id: RoomId,
    vote_id: VoteId,
) -> Result<()> {
    redis::Script::new(CLEANUP_SCRIPT)
        .key(CurrentVoteIdKey { room_id })
        .key(VoteCountKey { room_id, vote_id })
        .key(VoteParametersKey { room_id, vote_id })
        .key(AllowedUsersKey { room_id, vote_id })
        .key(ProtocolKey { room_id, vote_id })
        .arg(vote_id)
        .invoke_async(redis_conn)
        .await
        .with_context(|| {
            format!(
                "Failed to cleanup vote room_id:{} vote_id:{}",
                room_id, vote_id
            )
        })
}

/// The user vote script
///
/// Casts a user vote through a Lua script that is executed on redis. The script ensures that the provided `vote id` equals
/// the currently active vote id.
///
/// The requesting user will be removed from the `allowed users list`, the script aborts when the removal fails.
///
/// When every check succeeds, the `vote count` for the corresponding vote option will be increased and the provided protocol
/// entry will be pushed to the `protocol`.
///
/// The following parameters have to be provided:
/// ```text
/// ARGV[1] = vote id
/// ARGV[2] = user id
/// ARGV[3] = protocol entry
/// ARGV[4] = vote option
///
/// KEYS[1] = current vote key
/// KEYS[2] = allowed users key
/// KEYS[3] = protocol key
/// KEYS[4] = vote count key
/// ```
const VOTE_SCRIPT: &str = r#"
if not (redis.call("get", KEYS[1]) == ARGV[1]) then
  return 1
end

if (redis.call("srem", KEYS[2], ARGV[2]) == 1) then
  redis.call("rpush", KEYS[3], ARGV[3])
  redis.call("zincrby", KEYS[4], 1, ARGV[4])
  return 0
else
  return 2
end
"#;

/// Mapping for codes that are returned by the [`VOTE_SCRIPT`]
pub(crate) enum VoteScriptResult {
    Success = 0,
    InvalidVoteId,
    Ineligible,
}

impl FromRedisValue for VoteScriptResult {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
        if let redis::Value::Int(val) = v {
            match val {
                0 => Ok(VoteScriptResult::Success),
                1 => Ok(VoteScriptResult::InvalidVoteId),
                2 => Ok(VoteScriptResult::Ineligible),
                _ => Err(redis::RedisError::from((
                    redis::ErrorKind::TypeError,
                    "Vote script must return int value between 0 and 2",
                ))),
            }
        } else {
            Err(redis::RedisError::from((
                redis::ErrorKind::TypeError,
                "Vote script must return int value",
            )))
        }
    }
}

/// Cast a vote for the specified option
///
/// The vote is done atomically on redis with a Lua script.
/// See [`VOTE_SCRIPT`] for more details.
#[tracing::instrument(
    name = "legalvote_cast_vote",
    skip(redis_conn, user_id, participant_id, vote_option)
)]
pub(crate) async fn vote(
    redis_conn: &mut ConnectionManager,
    room_id: RoomId,
    vote_id: VoteId,
    user_id: UserId,
    participant_id: ParticipantId,
    vote_option: VoteOption,
) -> Result<VoteScriptResult> {
    let entry = ProtocolEntry::new(user_id, participant_id, VoteEvent::Vote(vote_option));

    redis::Script::new(VOTE_SCRIPT)
        .key(CurrentVoteIdKey { room_id })
        .key(AllowedUsersKey { room_id, vote_id })
        .key(ProtocolKey { room_id, vote_id })
        .key(VoteCountKey { room_id, vote_id })
        .arg(vote_id)
        .arg(user_id)
        .arg(entry)
        .arg(vote_option)
        .invoke_async(redis_conn)
        .await
        .context("Failed to cast vote")
}
