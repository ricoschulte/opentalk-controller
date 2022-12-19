use anyhow::{anyhow, bail};
use anyhow::{Context, Result};
use controller::prelude::*;
use db_storage::legal_votes::types::protocol::v1::{ProtocolEntry, VoteEvent};
use db_storage::legal_votes::types::{Token, VoteKind, VoteOption};
use db_storage::legal_votes::LegalVoteId;
use db_storage::users::UserId;
use redis::AsyncCommands;
use redis_args::ToRedisArgs;
use std::collections::HashMap;

use crate::outgoing::VotingRecord;

/// Contains the vote protocol. The vote protocol is a list of [`ProtocolEntries`](ProtocolEntry)
/// with information about the event that happened.
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room_id}:vote={legal_vote_id}:protocol")]
pub(super) struct ProtocolKey {
    pub(super) room_id: SignalingRoomId,
    pub(super) legal_vote_id: LegalVoteId,
}

/// Add an entry to the vote protocol of `legal_vote_id`
#[tracing::instrument(name = "legal_vote_add_protocol_entry", skip(redis_conn, entry))]
pub(crate) async fn add_entry(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
    legal_vote_id: LegalVoteId,
    entry: ProtocolEntry,
) -> Result<()> {
    redis_conn
        .rpush(
            ProtocolKey {
                room_id,
                legal_vote_id,
            },
            entry,
        )
        .await
        .context("Failed to add vote protocol entry")?;

    Ok(())
}

/// Get the vote protocol for `legal_vote_id`
#[tracing::instrument(name = "legal_vote_get_protocol", skip(redis_conn))]
pub(crate) async fn get(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
    legal_vote_id: LegalVoteId,
) -> Result<Vec<ProtocolEntry>> {
    redis_conn
        .lrange(
            ProtocolKey {
                room_id,
                legal_vote_id,
            },
            0,
            -1,
        )
        .await
        .context("Failed to get vote protocol")
}

/// Extracts the voting record from protocol entries
///
/// Returns a Result of [VotingRecord] matching the kind of vote
#[tracing::instrument(name = "extract_voting_record_from_protocol", skip(protocol))]
pub(crate) fn extract_voting_record_from_protocol(
    protocol: &[ProtocolEntry],
) -> Result<VotingRecord> {
    // get the vote kind
    let kind = protocol
        .iter()
        .find_map(|entry| {
            if let VoteEvent::Start(start) = &entry.event {
                Some(start.parameters.inner.kind)
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow!("Missing 'Start' entry in legal vote protocol"))?;

    let vote_iter = protocol.iter().filter_map(|entry| {
        if let VoteEvent::Vote(ref vote) = entry.event {
            Some(vote)
        } else {
            None
        }
    });

    match kind {
        VoteKind::RollCall => {
            let voters = vote_iter
                .map(|vote| {
                    if let Some(ref user) = vote.user_info {
                        Ok((user.issuer, vote.option))
                    } else {
                        bail!("Legal vote protocol contains inconsistent vote entries");
                    }
                })
                .collect::<Result<HashMap<UserId, VoteOption>>>()?;
            Ok(VotingRecord::UserVotes(voters))
        }
        VoteKind::Pseudonymous => {
            let tokens = vote_iter
                .map(|vote| {
                    if vote.user_info.is_none() {
                        Ok((vote.token, vote.option))
                    } else {
                        bail!("Legal vote protocol contains inconsistent vote entries");
                    }
                })
                .collect::<Result<HashMap<Token, VoteOption>>>()?;
            Ok(VotingRecord::TokenVotes(tokens))
        }
    }
}
