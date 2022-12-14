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
    use controller::prelude::serde_json;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn global_serialize() {
        let produced = serde_json::to_value(&Message::MessageSent(MessageSent {
            id: MessageId::nil(),
            source: ParticipantId::nil(),
            content: "Hello All!".to_string(),
            scope: Scope::Global,
        }))
        .unwrap();

        let expected = json!({
            "message": "message_sent",
            "id": "00000000-0000-0000-0000-000000000000",
            "source": "00000000-0000-0000-0000-000000000000",
            "content": "Hello All!",
            "scope": "global"
        });

        assert_eq!(expected, produced);
    }

    #[test]
    fn group_serialize() {
        let produced = serde_json::to_value(&Message::MessageSent(MessageSent {
            id: MessageId::nil(),
            source: ParticipantId::nil(),
            content: "Hello managers!".to_string(),
            scope: Scope::Group("management".to_string()),
        }))
        .unwrap();
        let expected = json!({
            "message":"message_sent",
            "id":"00000000-0000-0000-0000-000000000000",
            "source":"00000000-0000-0000-0000-000000000000",
            "content":"Hello managers!",
            "scope":"group",
            "target":"management",
        });
        assert_eq!(expected, produced);
    }

    #[test]
    fn private_serialize() {
        let produced = serde_json::to_value(&Message::MessageSent(MessageSent {
            id: MessageId::nil(),
            source: ParticipantId::nil(),
            content: "Hello All!".to_string(),
            scope: Scope::Private(ParticipantId::new_test(1)),
        }))
        .unwrap();

        let expected = json!({
            "message": "message_sent",
            "id": "00000000-0000-0000-0000-000000000000",
            "source": "00000000-0000-0000-0000-000000000000",
            "content": "Hello All!",
            "scope": "private",
            "target": "00000000-0000-0000-0000-000000000001",
        });
        assert_eq!(expected, produced);
    }

    #[test]
    fn error_serialize() {
        let produced = serde_json::to_value(&Message::Error(Error::ChatDisabled)).unwrap();
        let expected = json!({
            "message": "error",
            "error": "chat_disabled",
        });
        assert_eq!(expected, produced);
    }
}
