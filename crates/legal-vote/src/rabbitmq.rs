use super::incoming::UserParameters;
use chrono::{DateTime, Utc};
use controller::db::legal_votes::VoteId;
use controller::prelude::*;
use serde::{Deserialize, Serialize};

/// Rabbitmq event to inform participants
#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Event {
    /// A new vote has started
    Start(Parameters),
    /// A vote has been stopped
    Stop(StopVote),
    /// A vote has been canceled
    Cancel(Cancel),
    /// The results for a vote have changed
    Update(VoteUpdate),
    /// A fatal internal server error has occurred
    FatalServerError,
}

/// Wraps the [`UserParameters`] with additional server side information
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Deserialize)]
pub(crate) struct Parameters {
    /// The participant id of the vote initiator
    pub(crate) initiator_id: ParticipantId,
    /// The unique id of this vote
    pub(crate) vote_id: VoteId,
    /// The time the vote got started
    pub(crate) start_time: DateTime<Utc>,
    /// Parameters set by the initiator
    #[serde(flatten)]
    pub(crate) inner: UserParameters,
}

impl_to_redis_args_se!(&Parameters);
impl_from_redis_value_de!(Parameters);

/// The specified vote has been stopped
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct StopVote {
    /// The id of the stopped vote
    pub(crate) vote_id: VoteId,
}

/// The specified vote has been canceled
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Cancel {
    /// The id of the canceled vote
    pub(crate) vote_id: VoteId,
    /// The Reason for the cancel
    pub(crate) reason: Reason,
}

/// The reason for the cancel
#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Reason {
    /// The room got destroyed and the server canceled the vote
    RoomDestroyed,
    /// The initiator left the room and the server canceled the vote
    InitiatorLeft,
    /// Custom reason for a cancel
    Custom(String),
}

/// The results for a vote have changed
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct VoteUpdate {
    /// The id of the affected vote
    pub(crate) vote_id: VoteId,
}
