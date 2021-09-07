use super::BreakoutRoomId;
use crate::api::signaling::ParticipantId;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Message {
    Start(Start),
    Stop,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Start {
    pub rooms: u32,
    #[serde(default, with = "time")]
    pub duration: Option<Duration>,
    #[serde(flatten)]
    pub strategy: Strategy,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "strategy", rename_all = "snake_case")]
pub enum Strategy {
    Manual {
        assignments: HashMap<ParticipantId, BreakoutRoomId>,
    },
    LetParticipantsChoose,
}

mod time {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let seconds: Option<u64> = Deserialize::deserialize(deserializer)?;
        Ok(seconds.map(Duration::from_secs))
    }

    pub fn serialize<S>(duration: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(duration) = duration {
            serializer.serialize_u64(duration.as_secs())
        } else {
            serializer.serialize_none()
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn break_out_let_participants_choose() {
        let json = r#"
        {
            "action": "start",
            "rooms": 3,
            "duration": 123454321,
            "strategy": "let_participants_choose"
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        match msg {
            Message::Start(Start {
                rooms,
                duration,
                strategy,
            }) => {
                assert_eq!(rooms, 3);
                assert_eq!(strategy, Strategy::LetParticipantsChoose);
                assert_eq!(duration, Some(Duration::from_secs(123454321)));
            }
            Message::Stop => panic!(),
        }
    }

    #[test]
    fn break_out_manual() {
        let json = r#"
        {
            "action": "start",
            "rooms": 3,
            "strategy": "manual",
            "assignments": {
                "00000000-0000-0000-0000-000000000000": 0
            }
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        match msg {
            Message::Start(Start {
                rooms,
                duration,
                strategy,
            }) => {
                assert_eq!(rooms, 3);
                match strategy {
                    Strategy::Manual { assignments } => {
                        assert_eq!(assignments.len(), 1);
                        assert_eq!(
                            assignments.get(&ParticipantId::nil()),
                            Some(&BreakoutRoomId::nil())
                        );
                    }
                    Strategy::LetParticipantsChoose => panic!(),
                }
                assert_eq!(duration, None);
            }
            Message::Stop => panic!(),
        }
    }
}
