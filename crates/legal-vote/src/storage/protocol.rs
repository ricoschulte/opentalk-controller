use anyhow::{Context, Result};
use controller::db::legal_votes::VoteId;
use controller::prelude::*;
use controller_shared::ParticipantId;
use db_storage::legal_votes::types::protocol::v1::{ProtocolEntry, VoteEvent};
use db_storage::legal_votes::types::VoteOption;
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use std::collections::HashMap;

#[derive(Display)]
/// k3k-signaling:room={room_id}:vote={vote_id}:protocol
#[ignore_extra_doc_attributes]
///
/// Contains the vote protocol. The vote protocol is a list of [`ProtocolEntries`](ProtocolEntry)
/// with information about the event that happened.
pub(super) struct ProtocolKey {
    pub(super) room_id: SignalingRoomId,
    pub(super) vote_id: VoteId,
}

impl_to_redis_args!(ProtocolKey);

/// Add an entry to the vote protocol of `vote_id`
#[tracing::instrument(name = "legalvote_add_protocol_entry", skip(redis_conn, entry))]
pub(crate) async fn add_entry(
    redis_conn: &mut ConnectionManager,
    room_id: SignalingRoomId,
    vote_id: VoteId,
    entry: ProtocolEntry,
) -> Result<()> {
    redis_conn
        .rpush(ProtocolKey { room_id, vote_id }, entry)
        .await
        .context("Failed to add vote protocol entry")?;

    Ok(())
}

/// Get the vote protocol for `vote_id`
#[tracing::instrument(name = "legalvote_get_protocol", skip(redis_conn))]
pub(crate) async fn get(
    redis_conn: &mut ConnectionManager,
    room_id: SignalingRoomId,
    vote_id: VoteId,
) -> Result<Vec<ProtocolEntry>> {
    redis_conn
        .lrange(ProtocolKey { room_id, vote_id }, 0, -1)
        .await
        .context("Failed to get vote protocol")
}

/// Filters the protocol for vote events and returns a list of [`Vote`] events
#[tracing::instrument(name = "legalvote_reduce_protocol", skip(protocol))]
pub(crate) fn reduce_protocol(protocol: Vec<ProtocolEntry>) -> HashMap<ParticipantId, VoteOption> {
    protocol
        .into_iter()
        .filter_map(|entry| {
            if let VoteEvent::Vote(vote) = entry.event {
                Some(vote)
            } else {
                None
            }
        })
        .map(|vote| (vote.participant_id, vote.option))
        .collect::<HashMap<ParticipantId, VoteOption>>()
}
