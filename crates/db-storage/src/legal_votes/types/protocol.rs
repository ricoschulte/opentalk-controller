use super::{CancelReason, FinalResults, Parameters, StopKind, VoteOption};
use crate::users::UserId;
use chrono::{DateTime, Utc};
use controller_shared::{impl_from_redis_value_de, impl_to_redis_args_se, ParticipantId};
use serde::{Deserialize, Serialize};

/// A legalvote protocol entry
///
/// Contains the event that will be logged in the vote protocol with some meta data.
#[derive(Debug, Serialize, PartialEq, Eq, Deserialize)]
pub struct ProtocolEntry {
    /// The time that an entry got created
    pub timestamp: DateTime<Utc>,
    /// The event of this entry
    pub event: VoteEvent,
}

impl_to_redis_args_se!(ProtocolEntry);
impl_from_redis_value_de!(ProtocolEntry);

impl ProtocolEntry {
    /// Create a new protocol entry
    pub fn new(event: VoteEvent) -> Self {
        Self::new_with_time(Utc::now(), event)
    }

    /// Create a new protocol entry using the provided `timestamp`
    pub fn new_with_time(timestamp: DateTime<Utc>, event: VoteEvent) -> Self {
        Self { timestamp, event }
    }
}

/// A event related to an active vote
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "event")]
pub enum VoteEvent {
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
pub struct Start {
    /// User id of the initiator
    pub issuer: UserId,
    /// Vote parameters
    pub parameters: Parameters,
}

/// A vote entry mapped to a specific user
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vote {
    /// The user id of the voting user
    pub issuer: UserId,
    /// The users participant id, used when reducing the protocol for the frontend
    pub participant_id: ParticipantId,
    /// The chosen vote option
    pub option: VoteOption,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cancel {
    /// The user id of the entry issuer
    pub issuer: UserId,
    /// The reason for the cancel
    #[serde(flatten)]
    pub reason: CancelReason,
}
