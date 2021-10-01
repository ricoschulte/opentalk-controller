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
    Started(super::rabbitmq::Parameters),
    /// Direct response to a previous vote request (see [`Vote`](super::incoming::Message::Vote))
    Voted(VoteResponse),
    /// The results of a specific vote have changed
    Updated(VoteResults),
    /// A vote has been stopped
    Stopped(VoteResults),
    /// A vote has been canceled
    Canceled(Canceled),
    /// A error message caused by invalid requests or internal errors
    Error(ErrorKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VoteResponse {
    pub vote_id: VoteId,
    pub response: Response,
}

/// The direct response to an issued vote request
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
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

/// The vote options with their respective vote count
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct Votes {
    /// Vote count for yes
    pub yes: u64,
    /// Vote count for no
    pub no: u64,
    /// Vote count for abstain, abstain has to be enabled in the vote parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstain: Option<u64>,
}

/// The results for a vote
#[derive(Debug, Serialize, PartialEq)]
pub struct VoteResults {
    /// The vote id
    pub vote_id: VoteId,
    /// The vote options with their respective vote count
    pub votes: Votes,
    /// A map of participants with their chosen vote option. None when vote is secret.
    pub voters: Option<HashMap<ParticipantId, VoteOption>>,
}

/// A cancel message
#[derive(Debug, Serialize, PartialEq)]
pub struct Canceled {
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
#[serde(rename_all = "snake_case", tag = "error")]
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
        let json_str = r#"{"message":"started","initiator_id":"00000000-0000-0000-0000-000000000000","vote_id":"00000000-0000-0000-0000-000000000000","start_time":"1970-01-01T00:00:00Z","name":"TestVote","topic":"Yes or No?","allowed_participants":["00000000-0000-0000-0000-000000000001","00000000-0000-0000-0000-000000000002"],"enable_abstain":false,"secret":false,"auto_stop":false,"duration":null}"#;

        let message = Message::Started(rabbitmq::Parameters {
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
        let json_str = r#"{"message":"voted","vote_id":"00000000-0000-0000-0000-000000000000","response":"success"}"#;

        let message = Message::Voted(VoteResponse {
            vote_id: VoteId::from(Uuid::nil()),
            response: Response::Success,
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn vote_failed_invalid_vote_id_message() {
        let json_str = r#"{"message":"voted","vote_id":"00000000-0000-0000-0000-000000000000","response":{"failed":{"reason":"invalid_vote_id"}}}"#;

        let message = Message::Voted(VoteResponse {
            vote_id: VoteId::from(Uuid::nil()),
            response: Response::Failed(VoteFailed::InvalidVoteId),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn vote_failed_ineligible_message() {
        let json_str = r#"{"message":"voted","vote_id":"00000000-0000-0000-0000-000000000000","response":{"failed":{"reason":"ineligible"}}}"#;

        let message = Message::Voted(VoteResponse {
            vote_id: VoteId::from(Uuid::nil()),
            response: Response::Failed(VoteFailed::Ineligible),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn vote_failed_invalid_option_message() {
        let json_str = r#"{"message":"voted","vote_id":"00000000-0000-0000-0000-000000000000","response":{"failed":{"reason":"invalid_option"}}}"#;

        let message = Message::Voted(VoteResponse {
            vote_id: VoteId::from(Uuid::nil()),
            response: Response::Failed(VoteFailed::InvalidOption),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn public_update_message() {
        let json_str = r#"{"message":"updated","vote_id":"00000000-0000-0000-0000-000000000000","votes":{"yes":1,"no":0},"voters":{"00000000-0000-0000-0000-000000000001":"yes"}}"#;

        let votes = Votes {
            yes: 1,
            no: 0,
            abstain: None,
        };

        let mut voters = HashMap::new();
        voters.insert(ParticipantId::new_test(1), VoteOption::Yes);

        let message = Message::Updated(VoteResults {
            vote_id: VoteId::from(Uuid::nil()),
            votes,
            voters: Some(voters),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn secret_update_message() {
        let json_str = r#"{"message":"updated","vote_id":"00000000-0000-0000-0000-000000000000","votes":{"yes":1,"no":0},"voters":null}"#;

        let votes = Votes {
            yes: 1,
            no: 0,
            abstain: None,
        };

        let message = Message::Updated(VoteResults {
            vote_id: VoteId::from(Uuid::nil()),
            votes,
            voters: None,
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn public_stop_message() {
        let json_str = r#"{"message":"stopped","vote_id":"00000000-0000-0000-0000-000000000000","votes":{"yes":1,"no":0},"voters":{"00000000-0000-0000-0000-000000000001":"yes"}}"#;

        let votes = Votes {
            yes: 1,
            no: 0,
            abstain: None,
        };

        let mut voters = HashMap::new();
        voters.insert(ParticipantId::new_test(1), VoteOption::Yes);

        let message = Message::Stopped(VoteResults {
            vote_id: VoteId::from(Uuid::nil()),
            votes,
            voters: Some(voters),
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn secret_stop_message() {
        let json_str = r#"{"message":"stopped","vote_id":"00000000-0000-0000-0000-000000000000","votes":{"yes":0,"no":1,"abstain":2},"voters":null}"#;

        let votes = Votes {
            yes: 0,
            no: 1,
            abstain: Some(2),
        };

        let message = Message::Stopped(VoteResults {
            vote_id: VoteId::from(Uuid::nil()),
            votes,
            voters: None,
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn room_destroyed_cancel_message() {
        let json_str = r#"{"message":"canceled","vote_id":"00000000-0000-0000-0000-000000000000","reason":"room_destroyed"}"#;

        let message = Message::Canceled(Canceled {
            vote_id: VoteId::from(Uuid::nil()),
            reason: Reason::RoomDestroyed,
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn initiator_left_cancel_message() {
        let json_str = r#"{"message":"canceled","vote_id":"00000000-0000-0000-0000-000000000000","reason":"initiator_left"}"#;

        let message = Message::Canceled(Canceled {
            vote_id: VoteId::from(Uuid::nil()),
            reason: Reason::InitiatorLeft,
        });

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str)
    }

    #[test]
    fn custom_cancel_message() {
        let json_str = r#"{"message":"canceled","vote_id":"00000000-0000-0000-0000-000000000000","reason":{"custom":"A custom reason"}}"#;

        let message = Message::Canceled(Canceled {
            vote_id: VoteId::from(Uuid::nil()),
            reason: Reason::Custom("A custom reason".into()),
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
    fn internal_error_message() {
        let json_str = r#"{"message":"error","error":"internal"}"#;

        let message = Message::Error(ErrorKind::Internal);

        let string = serde_json::to_string(&message).unwrap();

        assert_eq!(string, json_str);
    }
}
