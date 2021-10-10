use super::VoteOption;
use controller::db::legal_votes::VoteId;
use controller::prelude::*;
use serde::{Deserialize, Serialize};

/// An incoming message issued by an participant
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum Message {
    /// Start a new vote
    Start(UserParameters),
    /// Stop a vote and show results to the participants
    Stop(Stop),
    /// Cancel a vote
    Cancel(Cancel),
    /// Vote for an item on a vote
    Vote(VoteMessage),
}

/// The users parameters to start a new vote
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Deserialize)]
pub struct UserParameters {
    /// The name of the vote
    pub name: String,
    /// The topic that will be voted on
    pub topic: String,
    /// List of participants that are allowed to cast a vote
    pub allowed_participants: Vec<ParticipantId>,
    /// Indicates that the `Abstain` vote option is enabled
    pub enable_abstain: bool,
    /// The vote will automatically stop when every participant voted
    pub auto_stop: bool,
    /// The vote will stop when the duration (in seconds) has passed
    pub duration: Option<u64>,
}

/// Stop a vote
#[derive(Debug, Clone, Deserialize)]
pub struct Stop {
    /// The vote id of the targeted vote
    pub vote_id: VoteId,
}

impl_to_redis_args_se!(UserParameters);
impl_from_redis_value_de!(UserParameters);

/// Cancel a vote
#[derive(Debug, Clone, Deserialize)]
pub struct Cancel {
    /// The vote id of the targeted vote
    pub vote_id: VoteId,
    /// The reason for the cancel
    pub reason: String,
}

/// Cast a vote
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct VoteMessage {
    /// The vote id of the targeted vote
    pub vote_id: VoteId,
    /// The chosen vote option
    pub option: VoteOption,
}

#[cfg(test)]
mod test {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn start_message() {
        let json_str = r#"
        {
            "action": "start",
            "name": "Vote Test",
            "topic": "Yes or No?",
            "allowed_participants": ["00000000-0000-0000-0000-000000000000"],
            "enable_abstain": false,
            "auto_stop": false,
            "duration": 60 
        }
        "#;

        let start: Message = serde_json::from_str(json_str).unwrap();

        if let Message::Start(UserParameters {
            name,
            topic,
            allowed_participants,
            enable_abstain,
            auto_stop,
            duration: time_in_sec,
        }) = start
        {
            assert_eq!("Vote Test", name);
            assert_eq!("Yes or No?", topic);
            assert_eq!(allowed_participants, vec![ParticipantId::nil()]);
            assert!(!enable_abstain);
            assert!(!auto_stop);
            assert_eq!(time_in_sec, Some(60));
        } else {
            panic!()
        }
    }

    #[test]
    fn stop_message() {
        let json_str = r#"
        {
            "action": "stop",
            "vote_id": "00000000-0000-0000-0000-000000000000"
        }
        "#;

        let stop: Message = serde_json::from_str(json_str).unwrap();

        if let Message::Stop(Stop { vote_id }) = stop {
            assert_eq!(vote_id, VoteId::from(Uuid::from_u128(0)))
        } else {
            panic!()
        }
    }

    #[test]
    fn cancel_message() {
        let json_str = r#"
        {
            "action": "cancel",
            "vote_id": "00000000-0000-0000-0000-000000000000",
            "reason": "Something is broken"
        }
        "#;

        let cancel: Message = serde_json::from_str(json_str).unwrap();

        if let Message::Cancel(Cancel { vote_id, reason }) = cancel {
            assert_eq!(vote_id, VoteId::from(Uuid::from_u128(0)));
            assert_eq!(reason, "Something is broken")
        } else {
            panic!()
        }
    }

    #[test]
    fn vote_yes_message() {
        let json_str = r#"
        {
            "action": "vote",
            "vote_id": "00000000-0000-0000-0000-000000000000",
            "option": "yes"
        }
        "#;

        let vote: Message = serde_json::from_str(json_str).unwrap();

        if let Message::Vote(VoteMessage { vote_id, option }) = vote {
            assert_eq!(vote_id, VoteId::from(Uuid::from_u128(0)));
            assert_eq!(option, VoteOption::Yes);
        } else {
            panic!()
        }
    }

    #[test]
    fn vote_no_message() {
        let json_str = r#"
        {
            "action": "vote",
            "vote_id": "00000000-0000-0000-0000-000000000000",
            "option": "no"
        }
        "#;

        let vote: Message = serde_json::from_str(json_str).unwrap();

        if let Message::Vote(VoteMessage { vote_id, option }) = vote {
            assert_eq!(vote_id, VoteId::from(Uuid::from_u128(0)));
            assert_eq!(option, VoteOption::No);
        } else {
            panic!()
        }
    }

    #[test]
    fn vote_abstain_message() {
        let json_str = r#"
        {
            "action": "vote",
            "vote_id": "00000000-0000-0000-0000-000000000000",
            "option": "abstain"
        }
        "#;

        let vote: Message = serde_json::from_str(json_str).unwrap();

        if let Message::Vote(VoteMessage { vote_id, option }) = vote {
            assert_eq!(vote_id, VoteId::from(Uuid::from_u128(0)));
            assert_eq!(option, VoteOption::Abstain);
        } else {
            panic!()
        }
    }
}
