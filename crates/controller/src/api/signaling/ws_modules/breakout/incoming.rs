// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use controller_shared::ParticipantId;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Message {
    Start(Start),
    Stop,
}

#[derive(Debug, Deserialize)]
pub struct Start {
    pub rooms: Vec<RoomParameter>,
    #[serde(default, with = "time")]
    pub duration: Option<Duration>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct RoomParameter {
    pub name: String,
    pub assignments: Vec<ParticipantId>,
}

mod time {
    use serde::{Deserialize, Deserializer};
    use std::time::Duration;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let seconds: Option<u64> = Deserialize::deserialize(deserializer)?;
        Ok(seconds.map(Duration::from_secs))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn breakout_start() {
        let json = r#"
        {
            "action": "start",
            "rooms": [
                { "name": "Room 1", "assignments": [] },
                { "name": "Room 2", "assignments": ["00000000-0000-0000-0000-000000000000"] }
            ],
            "duration": 123454321
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        match msg {
            Message::Start(Start { rooms, duration }) => {
                assert_eq!(
                    rooms,
                    vec![
                        RoomParameter {
                            name: "Room 1".into(),
                            assignments: vec![],
                        },
                        RoomParameter {
                            name: "Room 2".into(),
                            assignments: vec![ParticipantId::nil()],
                        }
                    ]
                );
                assert_eq!(duration, Some(Duration::from_secs(123454321)));
            }
            Message::Stop => panic!(),
        }
    }
}
