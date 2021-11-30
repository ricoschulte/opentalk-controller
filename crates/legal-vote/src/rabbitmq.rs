use super::incoming::UserParameters;
use crate::{outgoing::Votes, VoteOption};
use chrono::{DateTime, Utc};
use controller::db::legal_votes::VoteId;
use controller::prelude::*;
use controller_shared::ParticipantId;
use serde::{Deserialize, Serialize};

/// Rabbitmq event to inform participants
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    /// A new vote has started
    Start(Parameters),
    /// A participant has successfully voted, the message gets dispatched to the underlying user id
    Voted(VoteSuccess),
    /// A vote has been stopped
    Stop(Stop),
    /// A vote has been canceled
    Cancel(Cancel),
    /// The results for a vote have changed
    Update(VoteUpdate),
    /// A fatal internal server error has occurred
    FatalServerError,
}

/// Wraps the [`UserParameters`] with additional server side information
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Deserialize)]
pub struct Parameters {
    /// The participant id of the vote initiator
    pub initiator_id: ParticipantId,
    /// The unique id of this vote
    pub vote_id: VoteId,
    /// The time the vote got started
    pub start_time: DateTime<Utc>,
    /// The maximum amount of votes possible
    pub max_votes: u32,
    /// Parameters set by the initiator
    #[serde(flatten)]
    pub inner: UserParameters,
}

impl_to_redis_args_se!(&Parameters);
impl_from_redis_value_de!(Parameters);

/// A participant has successfully voted
///
/// This gets send to all participants that are participating with the same underlying user_id
#[derive(Debug, Serialize, Deserialize)]
pub struct VoteSuccess {
    /// The vote id
    pub vote_id: VoteId,
    /// The participant that issued the vote cast
    pub issuer: ParticipantId,
    /// The chosen vote option
    pub vote_option: VoteOption,
}

/// The specified vote has been stopped
#[derive(Debug, Serialize, Deserialize)]
pub struct Stop {
    /// The id of the stopped vote
    pub vote_id: VoteId,
    /// The kind of stop
    #[serde(flatten)]
    pub kind: StopKind,
    /// The final vote results
    pub results: FinalResults,
}

/// Describes the type of the vote stop
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "issuer")]
pub enum StopKind {
    /// A normal vote stop issued by a participant. Contains the ParticipantId of the issuer
    ByParticipant(ParticipantId),
    /// The vote has been stopped automatically because all allowed users have voted
    Auto,
    /// The vote expired due to a set duration
    Expired,
}

/// Final vote results
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "results")]
pub enum FinalResults {
    /// Valid vote results
    Valid(Votes),
    /// Invalid vote results
    Invalid(Invalid),
}

/// Describes the reason for invalid vote results
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum Invalid {
    /// An abstain vote was found when the vote itself has abstain disabled
    AbstainDisabled,
    /// The protocols vote count is not equal to the votes vote count
    VoteCountInconsistent,
}

/// The specified vote has been canceled
#[derive(Debug, Serialize, Deserialize)]
pub struct Cancel {
    /// The id of the canceled vote
    pub vote_id: VoteId,
    /// The reason for the cancel
    #[serde(flatten)]
    pub reason: CancelReason,
}

/// The reason for the cancel
#[derive(Debug, Clone, Eq, PartialOrd, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "reason", content = "custom")]
pub enum CancelReason {
    /// The room got destroyed and the server canceled the vote
    RoomDestroyed,
    /// The initiator left the room and the server canceled the vote
    InitiatorLeft,
    /// Custom reason for a cancel
    Custom(String),
}

/// The results for a vote have changed
#[derive(Debug, Serialize, Deserialize)]
pub struct VoteUpdate {
    /// The id of the affected vote
    pub vote_id: VoteId,
}
