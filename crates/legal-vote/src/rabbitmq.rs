use crate::outgoing::{self, PdfAsset};
use controller_shared::ParticipantId;
use db_storage::legal_votes::types::{CancelReason, FinalResults, Parameters, VoteOption};
use db_storage::legal_votes::LegalVoteId;
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
    Stop(outgoing::Stopped),
    /// A vote has been canceled
    Cancel(Canceled),
    /// The results for a vote have changed
    Update(VoteUpdate),
    /// A fatal internal server error has occurred
    FatalServerError,

    PdfAsset(PdfAsset),
}

/// A participant has successfully voted
///
/// This gets send to all participants that are participating with the same underlying user_id
#[derive(Debug, Serialize, Deserialize)]
pub struct VoteSuccess {
    /// The vote id
    pub legal_vote_id: LegalVoteId,
    /// The participant that issued the vote cast
    pub issuer: ParticipantId,
    /// The chosen vote option
    pub vote_option: VoteOption,
}

/// The specified vote has been stopped
#[derive(Debug, Serialize, Deserialize)]
pub struct Stop {
    /// The id of the stopped vote
    pub legal_vote_id: LegalVoteId,
    /// The kind of stop
    #[serde(flatten)]
    pub kind: StopKind,
    /// The final vote results
    pub results: FinalResults,
}

/// Describes the type of a vote stop
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

/// The specified vote has been canceled
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Canceled {
    /// The id of the canceled vote
    pub legal_vote_id: LegalVoteId,
    /// The reason for the cancel
    #[serde(flatten)]
    pub reason: CancelReason,
}

/// The results for a vote have changed
#[derive(Debug, Serialize, Deserialize)]
pub struct VoteUpdate {
    /// The id of the affected vote
    pub legal_vote_id: LegalVoteId,
}
