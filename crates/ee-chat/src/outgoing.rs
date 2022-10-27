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
    use controller::prelude::serde_json;
    use serde_json::json;

    #[test]
    fn message_sent_serialize() {
        let produced = serde_json::to_value(&Message::MessageSent(MessageSent {
            id: MessageId::nil(),
            source: ParticipantId::nil(),
            group: "management".to_string(),
            content: "Hello managers!".to_string(),
        }))
        .unwrap();
        let expected = json!({
            "message": "message_sent",
            "id": "00000000-0000-0000-0000-000000000000",
            "source": "00000000-0000-0000-0000-000000000000",
            "group": "management",
            "content": "Hello managers!",
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
