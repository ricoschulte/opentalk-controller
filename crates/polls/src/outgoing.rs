use crate::{Choice, ChoiceId, PollId};
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "message", rename_all = "snake_case")]
pub enum Message {
    Started(Started),
    LiveUpdate(Results),
    Done(Results),
    Error(Error),
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Started {
    pub id: PollId,
    pub topic: String,
    pub live: bool,
    pub choices: Vec<Choice>,
    #[serde(with = "super::duration_secs")]
    pub duration: Duration,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Results {
    pub id: PollId,
    pub results: Vec<Item>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Item {
    pub id: ChoiceId,
    pub count: u32,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "error")]
pub enum Error {
    InsufficientPermissions,
    InvalidChoiceCount,
    InvalidPollId,
    InvalidChoiceId,
    InvalidChoiceDescription,
    InvalidDuration,
    InvalidTopicLength,
    VotedAlready,
    StillRunning,
}

#[cfg(test)]
mod test {
    use super::*;
    use controller::prelude::*;
    use test_util::assert_eq_json;
    use uuid::Uuid;

    #[test]
    fn started() {
        let started = Message::Started(Started {
            id: PollId(Uuid::nil()),
            topic: "polling".into(),
            live: true,
            choices: vec![
                Choice {
                    id: ChoiceId(0),
                    content: "yes".into(),
                },
                Choice {
                    id: ChoiceId(1),
                    content: "no".into(),
                },
            ],
            duration: Duration::from_millis(10000),
        });

        assert_eq_json!(
          started,
          {
              "message": "started",
              "id": "00000000-0000-0000-0000-000000000000",
              "topic": "polling",
              "live": true,
              "choices": [
                  {
                      "id": 0,
                      "content": "yes"
                  },
                  {
                      "id": 1,
                      "content": "no"
                  }
              ],
              "duration": 10
          }
        );
    }

    #[test]
    fn live_update() {
        let live_update = Message::LiveUpdate(Results {
            id: PollId(Uuid::nil()),
            results: vec![
                Item {
                    id: ChoiceId(0),
                    count: 32,
                },
                Item {
                    id: ChoiceId(1),
                    count: 64,
                },
            ],
        });

        assert_eq_json!(
          live_update,
          {
              "message": "live_update",
              "id": "00000000-0000-0000-0000-000000000000",
              "results": [
                  {
                      "id": 0,
                      "count": 32
                  },
                  {
                      "id": 1,
                      "count": 64
                  }
              ]
          }
        );
    }

    #[test]
    fn done() {
        let done = Message::Done(Results {
            id: PollId(Uuid::nil()),
            results: vec![
                Item {
                    id: ChoiceId(0),
                    count: 32,
                },
                Item {
                    id: ChoiceId(1),
                    count: 64,
                },
            ],
        });

        assert_eq_json!(
          done,
          {
              "message": "done",
              "id": "00000000-0000-0000-0000-000000000000",
              "results": [
                  {
                      "id": 0,
                      "count": 32
                  },
                  {
                      "id": 1,
                      "count": 64
                  }
              ]
          }
        );
    }
}
