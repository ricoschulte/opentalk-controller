use crate::VoteOption;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use controller::db::legal_votes::VoteId;
use controller::db::rooms::RoomId;
use controller::db::users::UserId;
use controller::prelude::*;

use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Display)]
/// k3k-signaling:room={room_id}:vote={vote_id}:protocol
#[ignore_extra_doc_attributes]
///
/// Contains the vote protocol. The vote protocol is a list of [`ProtocolEntries`](ProtocolEntry)
/// with information about the event that happened.
pub(super) struct ProtocolKey {
    pub(super) room_id: RoomId,
    pub(super) vote_id: VoteId,
}

impl_to_redis_args!(ProtocolKey);

/// A protocol entry
///
/// Contains the event that will be logged in the vote protocol with some meta data.
#[derive(Debug, Serialize, PartialEq, Eq, Deserialize)]
pub(crate) struct ProtocolEntry {
    /// The user id of the entry issuer
    pub(crate) user_id: UserId,
    /// The participant id if the entry issuer
    pub(crate) participant_id: ParticipantId,
    /// The time that an entry got created
    pub(crate) timestamp: DateTime<Utc>,
    /// The event of this entry
    pub(crate) event: VoteEvent,
}

impl_to_redis_args_se!(ProtocolEntry);
impl_from_redis_value_de!(ProtocolEntry);

impl ProtocolEntry {
    /// Create a new protocol entry
    pub(crate) fn new(user_id: UserId, participant_id: ParticipantId, event: VoteEvent) -> Self {
        Self::new_with_time(Utc::now(), user_id, participant_id, event)
    }

    /// Create a new protocol entry using the provided `timestamp`
    pub(crate) fn new_with_time(
        timestamp: DateTime<Utc>,
        user_id: UserId,
        participant_id: ParticipantId,
        event: VoteEvent,
    ) -> Self {
        Self {
            user_id,
            participant_id,
            timestamp,
            event,
        }
    }
}

/// A event related to an active vote
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum VoteEvent {
    /// The vote started
    Start(crate::rabbitmq::Parameters),
    /// A vote has been casted
    Vote(VoteOption),
    /// The vote has been stopped
    Stop,
    /// The vote has been canceled
    Cancel(Reason),
}

impl_to_redis_args_se!(VoteEvent);

/// Reason for the cancel
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Reason {
    /// The room got destroyed and the server canceled the vote
    RoomDestroyed,
    /// The initiator left the room and the server canceled the vote
    InitiatorLeft,
    /// Custom reason for a cancel
    Custom(String),
}

/// Add an entry to the vote protocol of `vote_id`
#[tracing::instrument(name = "legalvote_add_protocol_entry", skip(redis_conn, entry))]
pub(crate) async fn add_entry(
    redis_conn: &mut ConnectionManager,
    room_id: RoomId,
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
    room_id: RoomId,
    vote_id: VoteId,
) -> Result<Vec<ProtocolEntry>> {
    redis_conn
        .lrange(ProtocolKey { room_id, vote_id }, 0, -1)
        .await
        .context("Failed to get vote protocol")
}

/// Filters the protocol for vote events and returns a map of participants with their chosen vote option
#[tracing::instrument(name = "legalvote_reduce_protocol", skip(protocol))]
pub(crate) fn reduce_protocol(protocol: Vec<ProtocolEntry>) -> HashMap<ParticipantId, VoteOption> {
    protocol
        .iter()
        .filter_map(|entry| {
            if let VoteEvent::Vote(option) = entry.event {
                Some((entry, option))
            } else {
                None
            }
        })
        .map(|(entry, option)| (entry.participant_id, option))
        .collect()
}
