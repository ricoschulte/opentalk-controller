use db_storage::legal_votes::types::{UserParameters, VoteOption};
use db_storage::legal_votes::LegalVoteId;
use serde::Deserialize;
use validator::Validate;

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

    /// Generate a PDF from a passed vote
    GeneratePdf(GeneratePdf),
}

impl Validate for Message {
    fn validate(&self) -> Result<(), validator::ValidationErrors> {
        match self {
            Message::Start(user_parameters) => user_parameters.validate(),
            Message::Stop(_) => Ok(()),
            Message::Cancel(cancel) => cancel.validate(),
            Message::Vote(_) => Ok(()),
            Message::GeneratePdf(_) => Ok(()),
        }
    }
}

/// Stop a vote
#[derive(Debug, Clone, Deserialize)]
pub struct Stop {
    /// The vote id of the targeted vote
    pub legal_vote_id: LegalVoteId,
}

/// Cancel a vote
#[derive(Debug, Clone, Deserialize, Validate)]
pub struct Cancel {
    /// The vote id of the targeted vote
    pub legal_vote_id: LegalVoteId,
    /// The reason for the cancel
    #[validate(length(max = 255))]
    pub reason: String,
}

/// Cast a vote
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct VoteMessage {
    /// The vote id of the targeted vote
    pub legal_vote_id: LegalVoteId,
    /// The chosen vote option
    pub option: VoteOption,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct GeneratePdf {
    pub legal_vote_id: LegalVoteId,
}

#[cfg(test)]
mod test {
    use super::*;
    use controller::prelude::*;
    use controller_shared::ParticipantId;
    use db_storage::legal_votes::types::VoteKind;
    use test_util::serde_json::json;
    use uuid::Uuid;

    #[test]
    fn start_message() {
        let json = json!(
            {
                "kind": "roll_call",
                "action": "start",
                "name": "Vote Test",
                "subtitle": "A subtitle",
                "topic": "Yes or No?",
                "allowed_participants": ["00000000-0000-0000-0000-000000000000"],
                "enable_abstain": false,
                "auto_close": false,
                "duration": 60,
                "create_pdf": false
            }
        );

        let start: Message = serde_json::from_value(json).unwrap();

        if let Message::Start(UserParameters {
            kind,
            name,
            subtitle,
            topic,
            allowed_participants,
            enable_abstain,
            auto_close,
            duration: time_in_sec,
            create_pdf,
        }) = start
        {
            assert_eq!(VoteKind::RollCall, kind);
            assert_eq!("Vote Test", name);
            assert_eq!("A subtitle", subtitle.unwrap());
            assert_eq!("Yes or No?", topic.unwrap());
            assert_eq!(allowed_participants, vec![ParticipantId::nil()]);
            assert!(!enable_abstain);
            assert!(!auto_close);
            assert_eq!(time_in_sec, Some(60));
            assert!(!create_pdf);
        } else {
            panic!()
        }
    }

    #[test]
    fn stop_message() {
        let json = json!(
            {
                "action": "stop",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000"
            }
        );

        let stop: Message = serde_json::from_value(json).unwrap();

        if let Message::Stop(Stop { legal_vote_id }) = stop {
            assert_eq!(legal_vote_id, LegalVoteId::from(Uuid::from_u128(0)))
        } else {
            panic!()
        }
    }

    #[test]
    fn cancel_message() {
        let json = json!(
            {
                "action": "cancel",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "reason": "Something is broken"
            }
        );

        let cancel: Message = serde_json::from_value(json).unwrap();

        if let Message::Cancel(Cancel {
            legal_vote_id,
            reason,
        }) = cancel
        {
            assert_eq!(legal_vote_id, LegalVoteId::from(Uuid::from_u128(0)));
            assert_eq!(reason, "Something is broken")
        } else {
            panic!()
        }
    }

    #[test]
    fn vote_yes_message() {
        let json = json!(
            {
                "action": "vote",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "option": "yes"
            }
        );

        let vote: Message = serde_json::from_value(json).unwrap();

        if let Message::Vote(VoteMessage {
            legal_vote_id,
            option,
        }) = vote
        {
            assert_eq!(legal_vote_id, LegalVoteId::from(Uuid::from_u128(0)));
            assert_eq!(option, VoteOption::Yes);
        } else {
            panic!()
        }
    }

    #[test]
    fn vote_no_message() {
        let json = json!(
            {
                "action": "vote",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "option": "no"
            }
        );

        let vote: Message = serde_json::from_value(json).unwrap();

        if let Message::Vote(VoteMessage {
            legal_vote_id,
            option,
        }) = vote
        {
            assert_eq!(legal_vote_id, LegalVoteId::from(Uuid::from_u128(0)));
            assert_eq!(option, VoteOption::No);
        } else {
            panic!()
        }
    }

    #[test]
    fn vote_abstain_message() {
        let json = json!(
            {
                "action": "vote",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "option": "abstain"
            }
        );

        let vote: Message = serde_json::from_value(json).unwrap();

        if let Message::Vote(VoteMessage {
            legal_vote_id,
            option,
        }) = vote
        {
            assert_eq!(legal_vote_id, LegalVoteId::from(Uuid::from_u128(0)));
            assert_eq!(option, VoteOption::Abstain);
        } else {
            panic!()
        }
    }

    #[test]
    fn invalid_start_message() {
        let string_151 = "X".repeat(151);
        let string_256 = "X".repeat(256);
        let string_501 = "X".repeat(501);

        let json = json!(
            {
                "kind": "roll_call",
                "action": "start",
                "name": string_151,
                "subtitle": string_256,
                "topic": string_501,
                "allowed_participants": [],
                "enable_abstain": false,
                "auto_close": false,
                "duration": 4,
                "create_pdf": false
            }
        );

        let start: Message = serde_json::from_value(json).unwrap();

        if let Err(validation_errors) = start.validate() {
            let errors = validation_errors.errors();

            assert!(errors.contains_key("name"));
            assert!(errors.contains_key("subtitle"));
            assert!(errors.contains_key("topic"));
            assert!(errors.contains_key("allowed_participants"));
            assert!(errors.contains_key("duration"));

            assert_eq!(errors.len(), 5);
        } else {
            panic!("Expected validation errors");
        }
    }

    #[test]
    fn invalid_cancel_message() {
        let string_256 = "X".repeat(256);

        let json = json!(
            {
                "action": "cancel",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "reason": string_256
            }
        );

        let cancel: Message = serde_json::from_value(json).unwrap();

        if let Err(validation_errors) = cancel.validate() {
            let errors = validation_errors.errors();

            assert!(errors.contains_key("reason"));

            assert_eq!(errors.len(), 1);
        } else {
            panic!("Expected validation errors");
        }
    }
}
