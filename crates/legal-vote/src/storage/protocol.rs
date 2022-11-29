use anyhow::anyhow;
use anyhow::{Context, Result};
use controller::prelude::*;
use controller_shared::ParticipantId;
use db_storage::legal_votes::types::protocol::v1::{ProtocolEntry, UserInfo, VoteEvent};
use db_storage::legal_votes::types::VoteOption;
use db_storage::legal_votes::LegalVoteId;
use displaydoc::Display;
use either::Either;
use redis::AsyncCommands;
use std::collections::HashMap;
#[derive(Display)]
/// k3k-signaling:room={room_id}:vote={legal_vote_id}:protocol
#[ignore_extra_doc_attributes]
///
/// Contains the vote protocol. The vote protocol is a list of [`ProtocolEntries`](ProtocolEntry)
/// with information about the event that happened.
pub(super) struct ProtocolKey {
    pub(super) room_id: SignalingRoomId,
    pub(super) legal_vote_id: LegalVoteId,
}

impl_to_redis_args!(ProtocolKey);

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

/// Filters the protocol for vote events
///
/// Returns a HashMap containing participants and their respective vote choice when the vote is public
///
/// Returns just the casted vote choices when the vote is configured to be hidden
#[tracing::instrument(name = "legal_vote_reduce_protocol", skip(protocol))]
pub(crate) fn reduce_protocol(
    protocol: Vec<ProtocolEntry>,
) -> Result<Either<HashMap<ParticipantId, VoteOption>, Vec<VoteOption>>> {
    // check if the vote is hidden
    let hidden = protocol
        .iter()
        .find_map(|entry| {
            if let VoteEvent::Start(start) = &entry.event {
                Some(start.parameters.inner.hidden)
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow!("Missing 'Start' entry in legal vote protocol"))?;

    let vote_list = protocol
        .into_iter()
        .filter_map(|entry| {
            if let VoteEvent::Vote(vote) = entry.event {
                Some(vote)
            } else {
                None
            }
        })
        .map(|vote| (vote.user_info, vote.option))
        .collect::<Vec<(Option<UserInfo>, VoteOption)>>();

    // avoid checking an empty list with Iterator::all]`
    if vote_list.is_empty() {
        return if hidden {
            Ok(Either::Right(vec![]))
        } else {
            Ok(Either::Left(HashMap::new()))
        };
    }

    if vote_list.iter().all(|(u, ..)| u.is_some()) {
        // public vote
        Ok(Either::Left(
            vote_list
                .into_iter()
                .map(|(u, v)| (u.unwrap().participant_id, v))
                .collect::<HashMap<ParticipantId, VoteOption>>(),
        ))
    } else if vote_list.iter().all(|(u, ..)| u.is_none()) {
        // hidden vote
        Ok(Either::Right(
            vote_list
                .into_iter()
                .map(|(_, v)| v)
                .collect::<Vec<VoteOption>>(),
        ))
    } else {
        // invalid vote
        Err(anyhow!(
            "Legal vote protocol contains inconsistent vote entries"
        ))
    }
}
