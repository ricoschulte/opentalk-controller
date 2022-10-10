use controller::prelude::*;
use controller_shared::ParticipantId;
use serde::{Deserialize, Serialize};

use crate::{MessageId, Scope};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "message", rename_all = "snake_case")]
pub enum Message {
    ChatEnabled(ChatEnabled),
    ChatDisabled(ChatDisabled),
    MessageSent(MessageSent),
    HistoryCleared(HistoryCleared),
    Error(Error),
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ChatEnabled {
    pub issued_by: ParticipantId,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ChatDisabled {
    pub issued_by: ParticipantId,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct MessageSent {
    pub id: MessageId,
    pub source: ParticipantId,
    pub content: String,
    #[serde(flatten)]
    pub scope: Scope,
    // todo The timestamp is now included in the Namespaced struct. Once the frontend adopted this change, remove the timestamp from MessageSent
    pub timestamp: Timestamp,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct HistoryCleared {
    pub issued_by: ParticipantId,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum Error {
    ChatDisabled,
    InsufficientPermissions,
}

#[cfg(test)]
mod test {
    use super::*;
    use controller::prelude::chrono::DateTime;
    use std::str::FromStr;

    #[test]
    fn global_serialize() {
        let produced = serde_json::to_string(&Message::MessageSent(MessageSent {
            id: MessageId::nil(),
            source: ParticipantId::nil(),
            timestamp: DateTime::from_str("2021-06-24T14:00:11.873753715Z")
                .unwrap()
                .into(),
            content: "Hello All!".to_string(),
            scope: Scope::Global,
        }))
        .unwrap();
        let expected = r#"{"message":"message_sent","id":"00000000-0000-0000-0000-000000000000","source":"00000000-0000-0000-0000-000000000000","content":"Hello All!","scope":"global","timestamp":"2021-06-24T14:00:11.873753715Z"}"#;

        assert_eq!(expected, produced);
    }

    #[test]
    fn private_serialize() {
        let produced = serde_json::to_string(&Message::MessageSent(MessageSent {
            id: MessageId::nil(),
            source: ParticipantId::nil(),
            timestamp: DateTime::from_str("2021-06-24T14:00:11.873753715Z")
                .unwrap()
                .into(),
            content: "Hello All!".to_string(),
            scope: Scope::Private(ParticipantId::new_test(1)),
        }))
        .unwrap();
        let expected = r#"{"message":"message_sent","id":"00000000-0000-0000-0000-000000000000","source":"00000000-0000-0000-0000-000000000000","content":"Hello All!","scope":"private","target":"00000000-0000-0000-0000-000000000001","timestamp":"2021-06-24T14:00:11.873753715Z"}"#;
        assert_eq!(expected, produced);
    }
}
