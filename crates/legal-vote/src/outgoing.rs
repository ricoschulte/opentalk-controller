use super::VoteOption;
use controller::db::legal_votes::VoteId;
use controller::prelude::*;
use serde::Serialize;
use std::collections::HashMap;

/// A message to the participant, send via a websocket connection
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "message")]
pub enum Message {
    /// Vote has started
    Start(super::rabbitmq::Parameters),
    /// Direct response to a previous vote request (see [`Vote`](super::incoming::Message::Vote))
    VoteResponse(VoteResponse),
    /// The results of a specific vote have changed
    Update(VoteResults),
    /// A vote has been stopped
    Stop(VoteResults),
    /// A vote has been canceled
    Cancel(Cancel),
    /// A error message caused by invalid requests or internal errors
    Error(ErrorKind),
}

/// The direct response to an issued vote request
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "response")]
pub enum VoteResponse {
    Success,
    Failed(VoteFailed),
}

/// Reasons for a failed vote request
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum VoteFailed {
    /// The given vote id is not active or does not exist
    InvalidVoteId,
    /// The requesting user already voted or is ineligible to vote
    Ineligible,
    /// Invalid vote option
    InvalidOption,
}

/// The results for a vote
#[derive(Debug, Serialize, PartialEq)]
pub struct VoteResults {
    /// The vote id
    pub vote_id: VoteId,
    /// The vote results
    pub results: Results,
}

/// The results may be public or secret depending on the vote parameters
#[derive(Debug, Serialize, PartialEq)]
pub enum Results {
    /// Public results can be mapped to a participant
    Public(PublicResults),
    /// Secret results contain only a vote count
    Secret(SecretResults),
}

/// Public vote results
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PublicResults {
    /// A map of [`VoteOption`] with their respective vote count
    pub votes: HashMap<VoteOption, u64>,
    /// A map of participants with their chosen vote option
    pub voters: HashMap<ParticipantId, VoteOption>,
}

/// Secret vote results
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SecretResults {
    /// A map of [`VoteOption`] with their respective vote count
    pub votes: HashMap<VoteOption, u64>,
}

/// A cancel message
#[derive(Debug, Serialize, PartialEq)]
pub struct Cancel {
    /// The vote that has been canceled
    pub vote_id: VoteId,
    /// The reason for the cancel
    pub reason: Reason,
}

/// The reason for the cancel
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// The room got destroyed and the server canceled the vote
    RoomDestroyed,
    /// The initiator left the room and the server canceled the vote
    InitiatorLeft,
    /// Custom reason for a cancel
    Custom(String),
}

impl From<super::rabbitmq::Reason> for Reason {
    fn from(reason: super::rabbitmq::Reason) -> Self {
        match reason {
            crate::rabbitmq::Reason::RoomDestroyed => Self::RoomDestroyed,
            crate::rabbitmq::Reason::InitiatorLeft => Self::InitiatorLeft,
            crate::rabbitmq::Reason::Custom(custom) => Self::Custom(custom),
        }
    }
}

/// The error kind sent to the user
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ErrorKind {
    /// A vote is already active
    VoteAlreadyActive,
    /// No vote is currently taking place
    NoVoteActive,
    /// The provided vote id is invalid in the requested context
    InvalidVoteId,
    /// The requesting user is ineligible to vote
    Ineligible,
    /// A internal server error occurred
    ///
    /// This means the legal-vote module is broken, the source of this is event are unrecoverable backend errors.
    Internal,
}

impl From<super::error::ErrorKind> for ErrorKind {
    fn from(vote_error: super::error::ErrorKind) -> Self {
        match vote_error {
            crate::error::ErrorKind::VoteAlreadyActive => Self::VoteAlreadyActive,
            crate::error::ErrorKind::NoVoteActive => Self::NoVoteActive,
            crate::error::ErrorKind::InvalidVoteId => Self::InvalidVoteId,
            crate::error::ErrorKind::Ineligible => Self::Ineligible,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{incoming, rabbitmq};
    use chrono::prelude::*;
    use uuid::Uuid;

    #[test]
    fn start_message() {
        let json_str = r#"{"message":"start","initiator_id":"00000000-0000-0000-0000-000000000000","vote_id":"00000000-0000-0000-0000-000000000000","start_time":"1970-01-01T00:00:00Z","name":"TestVote","topic":"Yes or No?","allowed_participants":["00000000-0000-0000-0000-000000000001","00000000-0000-0000-0000-000000000002"],"enable_abstain":false,"secret":false,"auto_stop":false,"duration":null}"#;

        let message = Message::Start(rabbitmq::Parameters {
            initiator_id: ParticipantId::nil(),
            vote_id: VoteId::from(Uuid::nil()),
            start_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
            inner: incoming::UserParameters {
                name: "TestVote".into(),
                topic: "Yes or No?".into(),
                allowed_participants: vec![ParticipantId::new_test(1), ParticipantId::new_test(2)],
                enable_abstain: false,
                secret: false,
                auto_stop: false,
                duration: None,
            },
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn vote_success_message() {
        let json_str = r#"{"message":"vote_response","response":"success"}"#;

        let message = Message::VoteResponse(VoteResponse::Success);

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn vote_failed_invalid_vote_id_message() {
        let json_str =
            r#"{"message":"vote_response","response":"failed","reason":"invalid_vote_id"}"#;

        let message = Message::VoteResponse(VoteResponse::Failed(VoteFailed::InvalidVoteId));

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn vote_failed_ineligible_message() {
        let json_str = r#"{"message":"vote_response","response":"failed","reason":"ineligible"}"#;

        let message = Message::VoteResponse(VoteResponse::Failed(VoteFailed::Ineligible));

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn vote_failed_invalid_option_message() {
        let json_str =
            r#"{"message":"vote_response","response":"failed","reason":"invalid_option"}"#;

        let message = Message::VoteResponse(VoteResponse::Failed(VoteFailed::InvalidOption));

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn public_update_message() {
        let json_str = r#"{"message":"update","vote_id":"00000000-0000-0000-0000-000000000000","results":{"Public":{"votes":{"yes":1},"voters":{"00000000-0000-0000-0000-000000000001":"yes"}}}}"#;

        let mut votes = HashMap::new();
        votes.insert(VoteOption::Yes, 1);

        let mut voters = HashMap::new();
        voters.insert(ParticipantId::new_test(1), VoteOption::Yes);

        let message = Message::Update(VoteResults {
            vote_id: VoteId::from(Uuid::nil()),
            results: Results::Public(PublicResults { votes, voters }),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn secret_update_message() {
        let json_str = r#"{"message":"update","vote_id":"00000000-0000-0000-0000-000000000000","results":{"Secret":{"votes":{"yes":1}}}}"#;

        let mut votes = HashMap::new();
        votes.insert(VoteOption::Yes, 1);

        let message = Message::Update(VoteResults {
            vote_id: VoteId::from(Uuid::nil()),
            results: Results::Secret(SecretResults { votes }),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn public_stop_message() {
        let json_str = r#"{"message":"stop","vote_id":"00000000-0000-0000-0000-000000000000","results":{"Public":{"votes":{"yes":1},"voters":{"00000000-0000-0000-0000-000000000001":"yes"}}}}"#;

        let mut votes = HashMap::new();
        votes.insert(VoteOption::Yes, 1);

        let mut voters = HashMap::new();
        voters.insert(ParticipantId::new_test(1), VoteOption::Yes);

        let message = Message::Stop(VoteResults {
            vote_id: VoteId::from(Uuid::nil()),
            results: Results::Public(PublicResults { votes, voters }),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn secret_stop_message() {
        let json_str = r#"{"message":"stop","vote_id":"00000000-0000-0000-0000-000000000000","results":{"Secret":{"votes":{"yes":1}}}}"#;

        let mut votes = HashMap::new();
        votes.insert(VoteOption::Yes, 1);

        let message = Message::Stop(VoteResults {
            vote_id: VoteId::from(Uuid::nil()),
            results: Results::Secret(SecretResults { votes }),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn room_destroyed_cancel_message() {
        let json_str = r#"{"message":"cancel","vote_id":"00000000-0000-0000-0000-000000000000","reason":"room_destroyed"}"#;

        let message = Message::Cancel(Cancel {
            vote_id: VoteId::from(Uuid::nil()),
            reason: Reason::RoomDestroyed,
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn initiator_left_cancel_message() {
        let json_str = r#"{"message":"cancel","vote_id":"00000000-0000-0000-0000-000000000000","reason":"initiator_left"}"#;

        let message = Message::Cancel(Cancel {
            vote_id: VoteId::from(Uuid::nil()),
            reason: Reason::InitiatorLeft,
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn custom_cancel_message() {
        let json_str = r#"{"message":"cancel","vote_id":"00000000-0000-0000-0000-000000000000","reason":{"custom":"A custom reason"}}"#;

        let message = Message::Cancel(Cancel {
            vote_id: VoteId::from(Uuid::nil()),
            reason: Reason::Custom("A custom reason".into()),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn ineligible_error_message() {
        let json_str = r#"{"message":"error","kind":"ineligible"}"#;

        let message = Message::Error(ErrorKind::Ineligible);

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str);
    }

    #[test]
    fn invalid_vote_id_error_message() {
        let json_str = r#"{"message":"error","kind":"invalid_vote_id"}"#;

        let message = Message::Error(ErrorKind::InvalidVoteId);

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str);
    }

    #[test]
    fn no_vote_active_error_message() {
        let json_str = r#"{"message":"error","kind":"no_vote_active"}"#;

        let message = Message::Error(ErrorKind::NoVoteActive);

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str);
    }

    #[test]
    fn vote_already_active_error_message() {
        let json_str = r#"{"message":"error","kind":"vote_already_active"}"#;

        let message = Message::Error(ErrorKind::VoteAlreadyActive);

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str);
    }

    #[test]
    fn internal_error_message() {
        let json_str = r#"{"message":"error","kind":"internal"}"#;

        let message = Message::Error(ErrorKind::Internal);

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str);
    }
}
