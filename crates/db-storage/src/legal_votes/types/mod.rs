use super::LegalVoteId;
use chrono::{DateTime, Utc};
use controller_shared::ParticipantId;
use redis_args::{FromRedisValue, ToRedisArgs};
use serde::{Deserialize, Serialize};
use validator::Validate;

pub mod protocol;

/// The vote choices
///
/// Abstain can be disabled through the vote parameters (See [`UserParameters`]).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToRedisArgs, FromRedisValue,
)]
#[serde(rename_all = "snake_case")]
#[to_redis_args(serde)]
#[from_redis_value(serde)]
pub enum VoteOption {
    Yes,
    No,
    Abstain,
}

/// Wraps the [`UserParameters`] with additional server side information
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Deserialize, ToRedisArgs, FromRedisValue)]
#[to_redis_args(serde)]
#[from_redis_value(serde)]
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

/// The users parameters to start a new vote
#[derive(
    Debug, Clone, Serialize, PartialEq, Eq, Deserialize, Validate, ToRedisArgs, FromRedisValue,
)]
#[to_redis_args(serde)]
#[from_redis_value(serde)]
pub struct UserParameters {
    /// The name of the vote
    #[validate(length(max = 150))]
    pub name: String,
    /// A Subtitle for the vote
    #[validate(length(max = 255))]
    pub subtitle: Option<String>,
    /// The topic that will be voted on
    #[validate(length(max = 500))]
    pub topic: Option<String>,
    /// List of participants that are allowed to cast a vote
    #[validate(length(min = 1))]
    pub allowed_participants: Vec<ParticipantId>,
    /// Indicates that the `Abstain` vote option is enabled
    pub enable_abstain: bool,
    /// Hide the participants vote choices from other participants
    pub hidden: bool,
    /// The vote will automatically stop when every participant voted
    pub auto_stop: bool,
    /// The vote will stop when the duration (in seconds) has passed
    #[validate(range(min = 5))]
    pub duration: Option<u64>,
    /// A PDF document will be created when the vote is over
    pub create_pdf: bool,
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

#[cfg(test)]
mod test {
    use super::*;
    use chrono::TimeZone;
    use controller_shared::ParticipantId;
    use test_util::assert_eq_json;
    use uuid::Uuid;

    #[test]
    fn serialize_parameters_with_optional_fields() {
        let params = Parameters {
            initiator_id: ParticipantId::nil(),
            legal_vote_id: LegalVoteId::from(Uuid::nil()),
            start_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
            max_votes: 2,
            inner: UserParameters {
                name: "TestWithOptionalFields".into(),
                subtitle: Some("A subtitle".into()),
                topic: Some("Yes or No?".into()),
                allowed_participants: vec![ParticipantId::new_test(1), ParticipantId::new_test(2)],
                enable_abstain: false,
                hidden: false,
                auto_stop: false,
                duration: Some(5u64),
                create_pdf: true,
            },
        };

        assert_eq_json!(
            params,
            {
                "initiator_id": "00000000-0000-0000-0000-000000000000",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "start_time": "1970-01-01T00:00:00Z",
                "max_votes": 2,
                "name": "TestWithOptionalFields",
                "subtitle": "A subtitle",
                "topic": "Yes or No?",
                "allowed_participants": [
                    "00000000-0000-0000-0000-000000000001",
                    "00000000-0000-0000-0000-000000000002"
                ],
                "enable_abstain": false,
                "hidden": false,
                "auto_stop": false,
                "duration": 5,
                "create_pdf": true
            }
        );
    }

    #[test]
    fn serialize_parameters_without_optional_fields() {
        let params = Parameters {
            initiator_id: ParticipantId::nil(),
            legal_vote_id: LegalVoteId::from(Uuid::nil()),
            start_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
            max_votes: 2,
            inner: UserParameters {
                name: "TestWithOptionalFields".into(),
                subtitle: None,
                topic: None,
                allowed_participants: vec![ParticipantId::new_test(1), ParticipantId::new_test(2)],
                enable_abstain: false,
                hidden: false,
                auto_stop: false,
                duration: None,
                create_pdf: true,
            },
        };

        assert_eq_json!(
            params,
            {
                "initiator_id": "00000000-0000-0000-0000-000000000000",
                "legal_vote_id": "00000000-0000-0000-0000-000000000000",
                "start_time": "1970-01-01T00:00:00Z",
                "max_votes": 2,
                "name": "TestWithOptionalFields",
                "subtitle": null,
                "topic": null,
                "allowed_participants": [
                    "00000000-0000-0000-0000-000000000001",
                    "00000000-0000-0000-0000-000000000002"
                ],
                "enable_abstain": false,
                "hidden": false,
                "auto_stop": false,
                "duration": null,
                "create_pdf": true
            }
        );
    }

    #[test]
    fn deserialize_parameters_with_optional_fields() {
        let json_str = r#"
        {
            "initiator_id": "00000000-0000-0000-0000-000000000000",
            "legal_vote_id": "00000000-0000-0000-0000-000000000000",
            "start_time": "1970-01-01T00:00:00Z",
            "max_votes": 2,
            "name": "Vote Test",
            "subtitle": "A subtitle",
            "topic": "Yes or No?",
            "allowed_participants": ["00000000-0000-0000-0000-000000000000"],
            "enable_abstain": false,
            "auto_stop": false,
            "hidden": false,
            "duration": 60,
            "create_pdf": true
        }
        "#;

        let params: Parameters = serde_json::from_str(json_str).unwrap();

        let Parameters {
            initiator_id,
            legal_vote_id,
            start_time,
            max_votes,
            inner:
                UserParameters {
                    name,
                    subtitle,
                    topic,
                    allowed_participants,
                    enable_abstain,
                    hidden,
                    auto_stop,
                    duration,
                    create_pdf,
                },
        } = params;

        assert_eq!(ParticipantId::nil(), initiator_id);
        assert_eq!(LegalVoteId::from(Uuid::nil()), legal_vote_id);
        assert_eq!(Utc.ymd(1970, 1, 1).and_hms(0, 0, 0), start_time);
        assert_eq!(2, max_votes);
        assert_eq!("Vote Test", name);
        assert_eq!("A subtitle", subtitle.unwrap());
        assert_eq!("Yes or No?", topic.unwrap());
        assert_eq!(allowed_participants, vec![ParticipantId::nil()]);
        assert!(!enable_abstain);
        assert!(!hidden);
        assert!(!auto_stop);
        assert_eq!(Some(60), duration);
        assert!(create_pdf);
    }

    #[test]
    fn deserialize_user_parameters_without_optional_fields() {
        let json_str = r#"
        {
            "initiator_id": "00000000-0000-0000-0000-000000000000",
            "legal_vote_id": "00000000-0000-0000-0000-000000000000",
            "start_time": "1970-01-01T00:00:00Z",
            "max_votes": 2,
            "name": "Vote Test",
            "subtitle": null,
            "topic": null,
            "allowed_participants": ["00000000-0000-0000-0000-000000000000"],
            "enable_abstain": false,
            "auto_stop": false,
            "hidden": false,
            "duration": null,
            "create_pdf": true
        }
        "#;

        let params: Parameters = serde_json::from_str(json_str).unwrap();

        let Parameters {
            initiator_id,
            legal_vote_id,
            start_time,
            max_votes,
            inner:
                UserParameters {
                    name,
                    subtitle,
                    topic,
                    allowed_participants,
                    enable_abstain,
                    hidden,
                    auto_stop,
                    duration,
                    create_pdf,
                },
        } = params;

        assert_eq!(ParticipantId::nil(), initiator_id);
        assert_eq!(LegalVoteId::from(Uuid::nil()), legal_vote_id);
        assert_eq!(Utc.ymd(1970, 1, 1).and_hms(0, 0, 0), start_time);
        assert_eq!(2, max_votes);
        assert_eq!("Vote Test", name);
        assert_eq!(None, subtitle);
        assert_eq!(None, topic);
        assert_eq!(allowed_participants, vec![ParticipantId::nil()]);
        assert!(!enable_abstain);
        assert!(!hidden);
        assert!(!auto_stop);
        assert_eq!(None, duration);
        assert!(create_pdf);
    }
}
