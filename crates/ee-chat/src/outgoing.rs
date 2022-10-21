use crate::chrono::{DateTime, Utc};
use chat::MessageId;
use controller_shared::ParticipantId;
use serde::{Deserialize, Serialize};

/// Message sent via websocket and rabbitmq
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "message", rename_all = "snake_case")]
pub enum Message {
    MessageSent(MessageSent),
    Error(Error),
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct MessageSent {
    pub id: MessageId,
    pub source: ParticipantId,
    pub group: String,
    // todo The timestamp is now included in the Namespaced struct. Once the frontends adopted this change, remove the timestamp from Message
    // See: https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/247
    pub timestamp: DateTime<Utc>,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum Error {
    ChatDisabled,
}

#[cfg(test)]
mod test {
    use super::*;
    use controller::prelude::{chrono::DateTime, serde_json};
    use serde_json::json;
    use std::str::FromStr;

    #[test]
    fn message_sent_serialize() {
        let produced = serde_json::to_value(&Message::MessageSent(MessageSent {
            id: MessageId::nil(),
            source: ParticipantId::nil(),
            group: "management".to_string(),
            timestamp: DateTime::from_str("2021-06-24T14:00:11.873753715Z").unwrap(),
            content: "Hello managers!".to_string(),
        }))
        .unwrap();
        let expected = json!({
            "message": "message_sent",
            "id": "00000000-0000-0000-0000-000000000000",
            "source": "00000000-0000-0000-0000-000000000000",
            "group": "management",
            "content": "Hello managers!",
            "timestamp":"2021-06-24T14:00:11.873753715Z",
        });

        assert_eq!(expected, produced);
    }

    #[test]
    fn error_serialize() {
        let produced = serde_json::to_value(&Message::Error(Error::ChatDisabled)).unwrap();
        let expected = json!({
            "message": "error",
            "error":"chat_disabled",
        });

        assert_eq!(expected, produced);
    }
}
