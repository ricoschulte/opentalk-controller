use super::VoteOption;
use crate::rabbitmq::{CancelReason, Invalid, Parameters, StopKind};
use controller::db::legal_votes::VoteId;
use controller::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A message to the participant, send via a websocket connection
#[derive(Debug, Serialize, PartialEq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VoteResponse {
    /// The vote id of the requested vote
    pub vote_id: VoteId,
    /// The response to the vote request
    #[serde(flatten)]
    pub response: Response,
}

/// Vote request response
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "response")]
pub enum Response {
    /// Response for a successful vote request
    Success(VoteSuccess),
    /// Response for a failed vote request
    Failed(VoteFailed),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct VoteSuccess {
    pub vote_option: VoteOption,
    pub issuer: ParticipantId,
}

/// Reasons for a failed vote request
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum VoteFailed {
    /// The given vote id is not active or does not exist
    InvalidVoteId,
    /// The requesting user already voted or is ineligible to vote. (requires the vote parameter `auto_stop` to be true)
    Ineligible,
    /// Invalid vote option
    InvalidOption,
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

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Results {
    /// The vote options with their respective vote count
    #[serde(flatten)]
    pub votes: Votes,
    /// A map of participants with their chosen vote option
    pub voters: HashMap<ParticipantId, VoteOption>,
}

/// The results for a vote
#[derive(Debug, Serialize, PartialEq)]
pub struct VoteResults {
    /// The vote id
    pub vote_id: VoteId,
    /// The vote results
    #[serde(flatten)]
    pub results: Results,
}

/// A stop message
#[derive(Debug, Serialize, PartialEq)]
pub struct Stopped {
    /// The vote id
    pub vote_id: VoteId,
    /// Specifies the reason for the stop
    #[serde(flatten)]
    pub kind: StopKind,
    /// The final vote results
    #[serde(flatten)]
    pub results: FinalResults,
}

/// The final results for a vote
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "results")]
pub enum FinalResults {
    /// Valid final results
    Valid(Results),
    /// Invalid final results
    Invalid(Invalid),
}

/// A cancel message
#[derive(Debug, Serialize, PartialEq)]
pub struct Canceled {
    /// The vote that has been canceled
    pub vote_id: VoteId,
    /// The reason for the cancel
    #[serde(flatten)]
    pub reason: CancelReason,
}

/// The error kind sent to the user
#[derive(Debug, PartialEq, Serialize)]
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
    /// A internal server error occurred
    ///
    /// This means the legal-vote module is broken, the source of this event are unrecoverable backend errors.
    Internal,
}

/// The list of provided guest participants.
#[derive(Debug, PartialEq, Serialize)]
pub struct GuestParticipants {
    pub guests: Vec<ParticipantId>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct InvalidFields {
    pub fields: Vec<String>,
}

impl From<super::error::ErrorKind> for ErrorKind {
    fn from(vote_error: super::error::ErrorKind) -> Self {
        match vote_error {
            crate::error::ErrorKind::VoteAlreadyActive => Self::VoteAlreadyActive,
            crate::error::ErrorKind::NoVoteActive => Self::NoVoteActive,
            crate::error::ErrorKind::InvalidVoteId => Self::InvalidVoteId,
            crate::error::ErrorKind::Ineligible => Self::Ineligible,
            crate::error::ErrorKind::Inconsistency => Self::Inconsistency,
            crate::error::ErrorKind::AllowlistContainsGuests(guests) => {
                Self::AllowlistContainsGuests(GuestParticipants { guests })
            }
            crate::error::ErrorKind::BadRequest(fields) => {
                Self::BadRequest(InvalidFields { fields })
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        incoming,
        rabbitmq::{self, CancelReason},
    };
    use chrono::prelude::*;
    use uuid::Uuid;

    #[test]
    fn start_message() {
        let json_str = r#"{"message":"started","initiator_id":"00000000-0000-0000-0000-000000000000","vote_id":"00000000-0000-0000-0000-000000000000","start_time":"1970-01-01T00:00:00Z","name":"TestVote","topic":"Yes or No?","allowed_participants":["00000000-0000-0000-0000-000000000001","00000000-0000-0000-0000-000000000002"],"enable_abstain":false,"auto_stop":false,"duration":null}"#;

        let message = Message::Started(rabbitmq::Parameters {
            initiator_id: ParticipantId::nil(),
            vote_id: VoteId::from(Uuid::nil()),
            start_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
            inner: incoming::UserParameters {
                name: "TestVote".into(),
                topic: "Yes or No?".into(),
                allowed_participants: vec![ParticipantId::new_test(1), ParticipantId::new_test(2)],
                enable_abstain: false,
                auto_stop: false,
                duration: None,
            },
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn vote_success_message() {
        let json_str = r#"{"message":"voted","vote_id":"00000000-0000-0000-0000-000000000000","response":"success","vote_option":"yes","issuer":"00000000-0000-0000-0000-000000000000"}"#;

        let message = Message::Voted(VoteResponse {
            vote_id: VoteId::from(Uuid::nil()),
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::Yes,
                issuer: ParticipantId::nil(),
            }),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn vote_failed_invalid_vote_id_message() {
        let json_str = r#"{"message":"voted","vote_id":"00000000-0000-0000-0000-000000000000","response":"failed","reason":"invalid_vote_id"}"#;

        let message = Message::Voted(VoteResponse {
            vote_id: VoteId::from(Uuid::nil()),
            response: Response::Failed(VoteFailed::InvalidVoteId),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn vote_failed_ineligible_message() {
        let json_str = r#"{"message":"voted","vote_id":"00000000-0000-0000-0000-000000000000","response":"failed","reason":"ineligible"}"#;

        let message = Message::Voted(VoteResponse {
            vote_id: VoteId::from(Uuid::nil()),
            response: Response::Failed(VoteFailed::Ineligible),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn vote_failed_invalid_option_message() {
        let json_str = r#"{"message":"voted","vote_id":"00000000-0000-0000-0000-000000000000","response":"failed","reason":"invalid_option"}"#;

        let message = Message::Voted(VoteResponse {
            vote_id: VoteId::from(Uuid::nil()),
            response: Response::Failed(VoteFailed::InvalidOption),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn update_message() {
        let json_str = r#"{"message":"updated","vote_id":"00000000-0000-0000-0000-000000000000","yes":1,"no":0,"voters":{"00000000-0000-0000-0000-000000000001":"yes"}}"#;

        let votes = Votes {
            yes: 1,
            no: 0,
            abstain: None,
        };

        let mut voters = HashMap::new();
        voters.insert(ParticipantId::new_test(1), VoteOption::Yes);

        let message = Message::Updated(VoteResults {
            vote_id: VoteId::from(Uuid::nil()),
            results: Results { votes, voters },
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn stop_message() {
        let json_str = r#"{"message":"stopped","vote_id":"00000000-0000-0000-0000-000000000000","kind":"by_participant","issuer":"00000000-0000-0000-0000-000000000000","results":"valid","yes":1,"no":0,"voters":{"00000000-0000-0000-0000-000000000001":"yes"}}"#;

        let votes = Votes {
            yes: 1,
            no: 0,
            abstain: None,
        };

        let mut voters = HashMap::new();
        voters.insert(ParticipantId::new_test(1), VoteOption::Yes);

        let message = Message::Stopped(Stopped {
            vote_id: VoteId::from(Uuid::nil()),
            kind: rabbitmq::StopKind::ByParticipant(ParticipantId::nil()),
            results: FinalResults::Valid(Results { votes, voters }),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn auto_stop_message() {
        let json_str = r#"{"message":"stopped","vote_id":"00000000-0000-0000-0000-000000000000","kind":"auto","results":"valid","yes":1,"no":0,"voters":{"00000000-0000-0000-0000-000000000001":"yes"}}"#;

        let votes = Votes {
            yes: 1,
            no: 0,
            abstain: None,
        };

        let mut voters = HashMap::new();
        voters.insert(ParticipantId::new_test(1), VoteOption::Yes);

        let message = Message::Stopped(Stopped {
            vote_id: VoteId::from(Uuid::nil()),
            kind: rabbitmq::StopKind::Auto,
            results: FinalResults::Valid(Results { votes, voters }),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn expired_stop_message() {
        let json_str = r#"{"message":"stopped","vote_id":"00000000-0000-0000-0000-000000000000","kind":"expired","results":"valid","yes":0,"no":0,"abstain":1,"voters":{"00000000-0000-0000-0000-000000000001":"abstain"}}"#;

        let votes = Votes {
            yes: 0,
            no: 0,
            abstain: Some(1),
        };

        let mut voters = HashMap::new();
        voters.insert(ParticipantId::new_test(1), VoteOption::Abstain);

        let message = Message::Stopped(Stopped {
            vote_id: VoteId::from(Uuid::nil()),
            kind: rabbitmq::StopKind::Expired,
            results: FinalResults::Valid(Results { votes, voters }),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn invalid_stop_message() {
        let json_str = r#"{"message":"stopped","vote_id":"00000000-0000-0000-0000-000000000000","kind":"by_participant","issuer":"00000000-0000-0000-0000-000000000000","results":"invalid","reason":"vote_count_inconsistent"}"#;

        let message = Message::Stopped(Stopped {
            vote_id: VoteId::from(Uuid::nil()),
            kind: rabbitmq::StopKind::ByParticipant(ParticipantId::nil()),
            results: FinalResults::Invalid(Invalid::VoteCountInconsistent),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn room_destroyed_cancel_message() {
        let json_str = r#"{"message":"canceled","vote_id":"00000000-0000-0000-0000-000000000000","reason":"room_destroyed"}"#;

        let message = Message::Canceled(Canceled {
            vote_id: VoteId::from(Uuid::nil()),
            reason: CancelReason::RoomDestroyed,
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn initiator_left_cancel_message() {
        let json_str = r#"{"message":"canceled","vote_id":"00000000-0000-0000-0000-000000000000","reason":"initiator_left"}"#;

        let message = Message::Canceled(Canceled {
            vote_id: VoteId::from(Uuid::nil()),
            reason: CancelReason::InitiatorLeft,
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn custom_cancel_message() {
        let json_str = r#"{"message":"canceled","vote_id":"00000000-0000-0000-0000-000000000000","reason":"custom","custom":"A custom reason"}"#;

        let message = Message::Canceled(Canceled {
            vote_id: VoteId::from(Uuid::nil()),
            reason: CancelReason::Custom("A custom reason".into()),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn ineligible_error_message() {
        let json_str = r#"{"message":"error","error":"ineligible"}"#;

        let message = Message::Error(ErrorKind::Ineligible);

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str);
    }

    #[test]
    fn invalid_vote_id_error_message() {
        let json_str = r#"{"message":"error","error":"invalid_vote_id"}"#;

        let message = Message::Error(ErrorKind::InvalidVoteId);

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str);
    }

    #[test]
    fn no_vote_active_error_message() {
        let json_str = r#"{"message":"error","error":"no_vote_active"}"#;

        let message = Message::Error(ErrorKind::NoVoteActive);

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str);
    }

    #[test]
    fn vote_already_active_error_message() {
        let json_str = r#"{"message":"error","error":"vote_already_active"}"#;

        let message = Message::Error(ErrorKind::VoteAlreadyActive);

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str);
    }

    #[test]
    fn allowlist_contains_guests_error_message() {
        let json_str = r#"{"message":"error","error":"allowlist_contains_guests","guests":["00000000-0000-0000-0000-000000000000","00000000-0000-0000-0000-000000000001"]}"#;

        let message = Message::Error(ErrorKind::AllowlistContainsGuests(GuestParticipants {
            guests: vec![ParticipantId::new_test(0), ParticipantId::new_test(1)],
        }));

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str);
    }

    #[test]
    fn bad_request_error_message() {
        let json_str = r#"{"message":"error","error":"bad_request","fields":["name","duration"]}"#;

        let message = Message::Error(ErrorKind::BadRequest(InvalidFields {
            fields: vec!["name".into(), "duration".into()],
        }));

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str);
    }

    #[test]
    fn internal_error_message() {
        let json_str = r#"{"message":"error","error":"internal"}"#;

        let message = Message::Error(ErrorKind::Internal);

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str);
    }
}
