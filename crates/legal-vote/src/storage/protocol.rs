use crate::error::{Error, ErrorKind};
use crate::rabbitmq::{CancelReason, FinalResults, StopKind};
use crate::VoteOption;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use controller::db::legal_votes::VoteId;
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
    pub(super) room_id: SignalingRoomId,
    pub(super) vote_id: VoteId,
}

impl_to_redis_args!(ProtocolKey);

/// A protocol entry
///
/// Contains the event that will be logged in the vote protocol with some meta data.
#[derive(Debug, Serialize, PartialEq, Eq, Deserialize)]
pub(crate) struct ProtocolEntry {
    /// The time that an entry got created
    pub(crate) timestamp: DateTime<Utc>,
    /// The event of this entry
    pub(crate) event: VoteEvent,
}

impl_to_redis_args_se!(ProtocolEntry);
impl_from_redis_value_de!(ProtocolEntry);

impl ProtocolEntry {
    /// Create a new protocol entry
    pub(crate) fn new(event: VoteEvent) -> Self {
        Self::new_with_time(Utc::now(), event)
    }

    /// Create a new protocol entry using the provided `timestamp`
    pub(crate) fn new_with_time(timestamp: DateTime<Utc>, event: VoteEvent) -> Self {
        Self { timestamp, event }
    }
}

/// A event related to an active vote
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "event")]
pub(crate) enum VoteEvent {
    /// The vote started
    Start(Start),
    /// A vote has been casted
    Vote(Vote),
    /// The vote has been stopped
    Stop(StopKind),
    /// The final vote results
    FinalResults(FinalResults),
    /// The vote has been canceled
    Cancel(Cancel),
}

impl_to_redis_args_se!(VoteEvent);

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Start {
    /// User id of the initiator
    pub(crate) issuer: UserId,
    /// Vote parameters
    pub(crate) parameters: crate::rabbitmq::Parameters,
}

/// A vote entry represents either a public or secret vote entry
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Vote {
    /// Public vote entry
    Public(PublicVote),
    /// Secret vote entry
    Secret(SecretVote),
}

impl Vote {
    pub(crate) fn get_vote_option(&self) -> VoteOption {
        match self {
            Vote::Public(public) => public.option,
            Vote::Secret(secret) => secret.option,
        }
    }
}

/// A public vote entry mapped to a specific user
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PublicVote {
    /// The user id of the voting user, `None` when the vote is secret
    pub(crate) issuer: UserId,
    /// The users participant id, used when reducing the protocol for the frontend
    pub(crate) participant_id: ParticipantId,
    /// The chosen vote option
    pub(crate) option: VoteOption,
}

/// A secret vote entry with no mapping to any user
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SecretVote {
    /// The chosen vote option
    pub(crate) option: VoteOption,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Cancel {
    /// The user id of the entry issuer
    pub(crate) issuer: UserId,
    /// The reason for the cancel
    pub(crate) reason: CancelReason,
}

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
pub(crate) fn reduce_public_protocol(
    protocol: Vec<ProtocolEntry>,
) -> Result<HashMap<ParticipantId, VoteOption>, Error> {
    protocol
        .into_iter()
        .filter_map(|entry| {
            if let VoteEvent::Vote(vote) = entry.event {
                Some(vote)
            } else {
                None
            }
        })
        .map(|vote| {
            if let Vote::Public(public_vote) = vote {
                Ok((public_vote.participant_id, public_vote.option))
            } else {
                Err(anyhow::Error::msg(
                    "Unexpected secret vote in public vote protocol",
                ))
            }
        })
        .collect::<Result<HashMap<ParticipantId, VoteOption>>>()
        .map_err(|_e| Error::Vote(ErrorKind::Inconsistency))
}
