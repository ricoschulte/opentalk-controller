// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::{ChoiceId, PollId};
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Message {
    Start(Start),
    Vote(Vote),
    Finish(Finish),
}

#[derive(Debug, Deserialize)]
pub struct Start {
    pub topic: String,
    pub live: bool,
    pub choices: Vec<String>,
    #[serde(with = "super::duration_secs")]
    pub duration: Duration,
}

#[derive(Debug, Deserialize)]
pub struct Vote {
    pub poll_id: PollId,
    pub choice_id: ChoiceId,
}

#[derive(Debug, Deserialize)]
pub struct Finish {
    pub id: PollId,
}

#[cfg(test)]
mod test {
    use super::*;
    use controller::prelude::*;
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    #[test]
    fn start() {
        let json = r#"
        {
            "action": "start",
            "topic": "abc",
            "live": true,
            "choices": ["a", "b", "c"],
            "duration": 30
        }
        "#;

        let message: Message = serde_json::from_str(json).unwrap();

        if let Message::Start(Start {
            topic,
            live,
            choices,
            duration,
        }) = message
        {
            assert_eq!(topic, "abc");
            assert!(live);
            assert_eq!(choices, vec!["a", "b", "c"]);
            assert_eq!(duration, Duration::from_secs(30));
        } else {
            panic!()
        }
    }

    #[test]
    fn vote() {
        let json = r#"
        {
            "action": "vote",
            "poll_id": "00000000-0000-0000-0000-000000000000",
            "choice_id": 321
         }
        "#;

        let message: Message = serde_json::from_str(json).unwrap();

        if let Message::Vote(Vote { poll_id, choice_id }) = message {
            assert_eq!(poll_id, PollId(Uuid::nil()));
            assert_eq!(choice_id, ChoiceId(321));
        } else {
            panic!()
        }
    }
}
