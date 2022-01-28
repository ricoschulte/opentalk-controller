use super::LegalVoteId;
use chrono::{DateTime, Utc};
use controller_shared::{impl_from_redis_value_de, impl_to_redis_args_se, ParticipantId};
use serde::{Deserialize, Serialize};
use validator::Validate;

pub mod protocol;

/// The vote choices
///
/// Abstain can be disabled through the vote parameters (See [`UserParameters`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoteOption {
    Yes,
    No,
    Abstain,
}

impl_to_redis_args_se!(VoteOption);
impl_from_redis_value_de!(VoteOption);

/// Wraps the [`UserParameters`] with additional server side information
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Deserialize)]
pub struct Parameters {
    /// The participant id of the vote initiator
    pub initiator_id: ParticipantId,
    /// The unique id of this vote
    pub legal_vote_id: LegalVoteId,
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

/// The users parameters to start a new vote
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Deserialize, Validate)]
pub struct UserParameters {
    /// The name of the vote
    #[validate(length(max = 150))]
    pub name: String,
    /// A Subtitle for the vote
    #[validate(length(max = 255))]
    pub subtitle: String,
    /// The topic that will be voted on
    #[validate(length(max = 500))]
    pub topic: String,
    /// List of participants that are allowed to cast a vote
    #[validate(length(min = 1))]
    pub allowed_participants: Vec<ParticipantId>,
    /// Indicates that the `Abstain` vote option is enabled
    pub enable_abstain: bool,
    /// The vote will automatically stop when every participant voted
    pub auto_stop: bool,
    /// The vote will stop when the duration (in seconds) has passed
    #[validate(range(min = 5))]
    pub duration: Option<u64>,
}

impl_to_redis_args_se!(UserParameters);
impl_from_redis_value_de!(UserParameters);

/// Final vote results
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "results")]
pub enum FinalResults {
    /// Valid vote results
    Valid(Votes),
    /// Invalid vote results
    Invalid(Invalid),
}

/// The vote options with their respective vote count
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Votes {
    /// Vote count for yes
    pub yes: u64,
    /// Vote count for no
    pub no: u64,
    /// Vote count for abstain, abstain has to be enabled in the vote parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstain: Option<u64>,
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

/// The reason for a cancel
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
