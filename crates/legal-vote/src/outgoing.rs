use crate::rabbitmq::{Canceled, StopKind};
use controller_shared::ParticipantId;
use db_storage::legal_votes::types::{Invalid, Parameters, VoteOption, Votes};
use db_storage::legal_votes::LegalVoteId;
use serde::Serialize;
use std::collections::HashMap;

/// A message to the participant, send via a websocket connection
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "message")]
pub enum Message {
    /// Vote has started
    Started(Parameters),
    /// Direct response to a previous vote request (see [`Vote`](super::incoming::Message::Vote))
    Voted(VoteResponse),
    /// The results of a specific vote have changed
    Updated(VoteResults),
    /// A vote has been stopped
    Stopped(Stopped),
    /// A vote has been canceled
    Canceled(Canceled),
    /// A error message caused by invalid requests or internal errors
    Error(ErrorKind),
}

/// The direct response to an issued vote request
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct VoteResponse {
    /// The vote id of the requested vote
    pub legal_vote_id: LegalVoteId,
    /// The response to the vote request
    #[serde(flatten)]
    pub response: Response,
}

/// Vote request response
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "response")]
pub enum Response {
    /// Response for a successful vote request
    Success(VoteSuccess),
    /// Response for a failed vote request
    Failed(VoteFailed),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct VoteSuccess {
    pub vote_option: VoteOption,
    pub issuer: ParticipantId,
}

/// Reasons for a failed vote request
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum VoteFailed {
    /// The given vote id is not active or does not exist
    InvalidVoteId,
    /// The requesting user already voted or is ineligible to vote. (requires the vote parameter `auto_stop` to be true)
    Ineligible,
    /// Invalid vote option
    InvalidOption,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Results {
    /// The vote options with their respective vote count
    #[serde(flatten)]
    pub votes: Votes,
    /// A map of participants with their chosen vote option
    pub voters: HashMap<ParticipantId, VoteOption>,
}

/// The results for a vote
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct VoteResults {
    /// The vote id
    pub legal_vote_id: LegalVoteId,
    /// The vote results
    #[serde(flatten)]
    pub results: Results,
}

/// A stop message
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Stopped {
    /// The vote id
    pub legal_vote_id: LegalVoteId,
    /// Specifies the reason for the stop
    #[serde(flatten)]
    pub kind: StopKind,
    /// The final vote results
    #[serde(flatten)]
    pub results: FinalResults,
}

/// The final results for a vote
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "results")]
pub enum FinalResults {
    /// Valid final results
    Valid(Results),
    /// Invalid final results
    Invalid(Invalid),
}

/// The error kind sent to the user
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "error")]
pub enum ErrorKind {
    /// A vote is already active
    VoteAlreadyActive,
    /// No vote is currently taking place
    NoVoteActive,
    /// The provided vote id is invalid in the requested context
    InvalidVoteId,
    /// The requesting user is ineligible
    Ineligible,
    /// The provided allow list contains guest participants
    AllowlistContainsGuests(GuestParticipants),
    /// A inconsistency occurred while handling a request
    Inconsistency,
    /// The provided parameters of a request are invalid
    BadRequest(InvalidFields),
    /// Failed to set or get permissions
    PermissionError,
    /// The requesting user has insufficent permissions
    InsufficentPermissions,
    /// A internal server error occurred
    ///
    /// This means the legal-vote module is broken, the source of this event are unrecoverable backend errors.
    Internal,
}

/// The list of provided guest participants.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct GuestParticipants {
    pub guests: Vec<ParticipantId>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct InvalidFields {
    pub fields: Vec<String>,
}

impl From<super::error::ErrorKind> for ErrorKind {
    fn from(vote_error: super::error::ErrorKind) -> Self {
        match vote_error {
            crate::error::ErrorKind::VoteAlreadyActive => Self::VoteAlreadyActive,
            crate::error::ErrorKind::NoVoteActive => Self::NoVoteActive,
            crate::error::ErrorKind::InvalidVoteId => Self::InvalidVoteId,
            crate::error::ErrorKind::Inconsistency => Self::Inconsistency,
            crate::error::ErrorKind::AllowlistContainsGuests(guests) => {
                Self::AllowlistContainsGuests(GuestParticipants { guests })
            }
            crate::error::ErrorKind::BadRequest(fields) => {
                Self::BadRequest(InvalidFields { fields })
            }
            crate::error::ErrorKind::PermissionError => Self::PermissionError,
            crate::error::ErrorKind::InsufficientPermissions => Self::InsufficentPermissions,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::prelude::*;
    use controller::prelude::*;
    use controller_shared::ParticipantId;
    use db_storage::legal_votes::types::{CancelReason, Parameters, UserParameters};
    use test_util::assert_eq_json;
    use uuid::Uuid;

    #[test]
    fn start_message() {
        let message = Message::Started(Parameters {
            initiator_id: ParticipantId::nil(),
            legal_vote_id: LegalVoteId::from(Uuid::nil()),
            start_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
            max_votes: 2,
            inner: UserParameters {
                name: "TestVote".into(),
                subtitle: "A subtitle".into(),
                topic: "Yes or No?".into(),
                allowed_participants: vec![ParticipantId::new_test(1), ParticipantId::new_test(2)],
                enable_abstain: false,
                auto_stop: false,
                duration: None,
            },
        });

        assert_eq_json!(
            message,
            {
                "message": "started",
                "initiator_id": "00000000-0000-0000-0000-000000000000",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "start_time": "1970-01-01T00:00:00Z",
                "max_votes": 2,
                "name": "TestVote",
                "subtitle": "A subtitle",
                "topic": "Yes or No?",
                "allowed_participants": [
                    "00000000-0000-0000-0000-000000000001",
                    "00000000-0000-0000-0000-000000000002"
                ],
                "enable_abstain": false,
                "auto_stop": false,
                "duration": null
            }
        );
    }

    #[test]
    fn vote_success_message() {
        let message = Message::Voted(VoteResponse {
            legal_vote_id: LegalVoteId::from(Uuid::nil()),
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::Yes,
                issuer: ParticipantId::nil(),
            }),
        });

        assert_eq_json!(
            message,
            {
                "message": "voted",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "response": "success",
                "vote_option": "yes",
                "issuer": "00000000-0000-0000-0000-000000000000"
            }
        );
    }

    #[test]
    fn vote_failed_invalid_vote_id_message() {
        let message = Message::Voted(VoteResponse {
            legal_vote_id: LegalVoteId::from(Uuid::nil()),
            response: Response::Failed(VoteFailed::InvalidVoteId),
        });

        assert_eq_json!(
            message,
            {
                "message": "voted",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "response": "failed",
                "reason": "invalid_vote_id"
            }
        );
    }

    #[test]
    fn vote_failed_ineligible_message() {
        let message = Message::Voted(VoteResponse {
            legal_vote_id: LegalVoteId::from(Uuid::nil()),
            response: Response::Failed(VoteFailed::Ineligible),
        });

        assert_eq_json!(
            message,
            {
                "message": "voted",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "response": "failed",
                "reason": "ineligible"
            }
        );
    }

    #[test]
    fn vote_failed_invalid_option_message() {
        let message = Message::Voted(VoteResponse {
            legal_vote_id: LegalVoteId::from(Uuid::nil()),
            response: Response::Failed(VoteFailed::InvalidOption),
        });

        assert_eq_json!(
            message,
            {
                "message": "voted",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "response": "failed",
                "reason": "invalid_option"
            }
        );
    }

    #[test]
    fn update_message() {
        let votes = Votes {
            yes: 1,
            no: 0,
            abstain: None,
        };

        let mut voters = HashMap::new();
        voters.insert(ParticipantId::new_test(1), VoteOption::Yes);

        let message = Message::Updated(VoteResults {
            legal_vote_id: LegalVoteId::from(Uuid::nil()),
            results: Results { votes, voters },
        });

        assert_eq_json!(
            message,
            {
                "message": "updated",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "yes": 1,
                "no": 0,
                "voters": {
                    "00000000-0000-0000-0000-000000000001": "yes"
                }
            }
        );
    }

    #[test]
    fn stop_message() {
        let votes = Votes {
            yes: 1,
            no: 0,
            abstain: None,
        };

        let mut voters = HashMap::new();
        voters.insert(ParticipantId::new_test(1), VoteOption::Yes);

        let message = Message::Stopped(Stopped {
            legal_vote_id: LegalVoteId::from(Uuid::nil()),
            kind: StopKind::ByParticipant(ParticipantId::nil()),
            results: FinalResults::Valid(Results { votes, voters }),
        });

        assert_eq_json!(
            message,
            {
                "message": "stopped",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "kind": "by_participant",
                "issuer": "00000000-0000-0000-0000-000000000000",
                "results": "valid",
                "yes": 1,
                "no": 0,
                "voters": {
                    "00000000-0000-0000-0000-000000000001": "yes"
                }
            }
        );
    }

    #[test]
    fn auto_stop_message() {
        let votes = Votes {
            yes: 1,
            no: 0,
            abstain: None,
        };

        let mut voters = HashMap::new();
        voters.insert(ParticipantId::new_test(1), VoteOption::Yes);

        let message = Message::Stopped(Stopped {
            legal_vote_id: LegalVoteId::from(Uuid::nil()),
            kind: StopKind::Auto,
            results: FinalResults::Valid(Results { votes, voters }),
        });

        assert_eq_json!(
            message,
            {
                "message": "stopped",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "kind": "auto",
                "results": "valid",
                "yes": 1,
                "no": 0,
                "voters": {
                    "00000000-0000-0000-0000-000000000001": "yes"
                }
            }
        );
    }

    #[test]
    fn expired_stop_message() {
        let votes = Votes {
            yes: 0,
            no: 0,
            abstain: Some(1),
        };

        let mut voters = HashMap::new();
        voters.insert(ParticipantId::new_test(1), VoteOption::Abstain);

        let message = Message::Stopped(Stopped {
            legal_vote_id: LegalVoteId::from(Uuid::nil()),
            kind: StopKind::Expired,
            results: FinalResults::Valid(Results { votes, voters }),
        });

        assert_eq_json!(
            message,
            {
                "message": "stopped",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "kind": "expired",
                "results": "valid",
                "yes": 0,
                "no": 0,
                "abstain": 1,
                "voters": {
                  "00000000-0000-0000-0000-000000000001": "abstain"
                }
            }
        );
    }

    #[test]
    fn invalid_stop_message() {
        let message = Message::Stopped(Stopped {
            legal_vote_id: LegalVoteId::from(Uuid::nil()),
            kind: StopKind::ByParticipant(ParticipantId::nil()),
            results: FinalResults::Invalid(Invalid::VoteCountInconsistent),
        });

        assert_eq_json!(
            message,
            {
                "message": "stopped",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "kind": "by_participant",
                "issuer": "00000000-0000-0000-0000-000000000000",
                "results": "invalid",
                "reason": "vote_count_inconsistent"
            }
        );
    }

    #[test]
    fn room_destroyed_cancel_message() {
        let message = Message::Canceled(Canceled {
            legal_vote_id: LegalVoteId::from(Uuid::nil()),
            reason: CancelReason::RoomDestroyed,
        });

        assert_eq_json!(
            message,
            {
                "message": "canceled",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "reason": "room_destroyed"
            }
        );
    }

    #[test]
    fn initiator_left_cancel_message() {
        let message = Message::Canceled(Canceled {
            legal_vote_id: LegalVoteId::from(Uuid::nil()),
            reason: CancelReason::InitiatorLeft,
        });

        assert_eq_json!(
            message,
            {
                "message": "canceled",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "reason": "initiator_left"
            }
        );
    }

    #[test]
    fn custom_cancel_message() {
        let message = Message::Canceled(Canceled {
            legal_vote_id: LegalVoteId::from(Uuid::nil()),
            reason: CancelReason::Custom("A custom reason".into()),
        });

        assert_eq_json!(
            message,
            {
                "message": "canceled",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "reason": "custom",
                "custom": "A custom reason"
            }
        );
    }

    #[test]
    fn ineligible_error_message() {
        let message = Message::Error(ErrorKind::Ineligible);

        assert_eq_json!(
            message,
            {
                "message": "error",
                "error": "ineligible"
            }
        );
    }

    #[test]
    fn invalid_vote_id_error_message() {
        let message = Message::Error(ErrorKind::InvalidVoteId);

        assert_eq_json!(
            message,
            {
                "message": "error",
                "error": "invalid_vote_id"
            }
        );
    }

    #[test]
    fn no_vote_active_error_message() {
        let message = Message::Error(ErrorKind::NoVoteActive);

        assert_eq_json!(
            message,
            {
                "message": "error",
                "error": "no_vote_active"
            }
        );
    }

    #[test]
    fn vote_already_active_error_message() {
        let message = Message::Error(ErrorKind::VoteAlreadyActive);

        assert_eq_json!(
            message,
            {
                "message": "error",
                "error": "vote_already_active"
            }
        );
    }

    #[test]
    fn allowlist_contains_guests_error_message() {
        let message = Message::Error(ErrorKind::AllowlistContainsGuests(GuestParticipants {
            guests: vec![ParticipantId::new_test(0), ParticipantId::new_test(1)],
        }));

        assert_eq_json!(
            message,
            {
                "message": "error",
                "error": "allowlist_contains_guests",
                "guests": [
                    "00000000-0000-0000-0000-000000000000",
                    "00000000-0000-0000-0000-000000000001"
                ]
            }
        );
    }

    #[test]
    fn bad_request_error_message() {
        let message = Message::Error(ErrorKind::BadRequest(InvalidFields {
            fields: vec!["name".into(), "duration".into()],
        }));

        assert_eq_json!(
            message,
            {
                "message": "error",
                "error": "bad_request",
                "fields": [
                    "name",
                    "duration"
                ]
            }
        );
    }

    #[test]
    fn internal_error_message() {
        let message = Message::Error(ErrorKind::Internal);

        assert_eq_json!(
            message,
            {
                "message": "error",
                "error": "internal",
            }
        );
    }

    #[test]
    fn insufficent_permissions_error_message() {
        let message = Message::Error(ErrorKind::InsufficentPermissions);

        assert_eq_json!(
            message,
            {
                "message": "error",
                "error": "insufficent_permissions",
            }
        );
    }
}
