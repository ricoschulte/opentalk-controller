use controller::prelude::*;
use controller_shared::ParticipantId;
use serde::Deserialize;

use crate::Scope;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Message {
    EnableChat,
    DisableChat,
    SendMessage {
        target: Option<ParticipantId>,
        content: String,
    },
    ClearHistory,
    SetLastSeenTimestamp {
        #[serde(flatten)]
        scope: Scope,
        timestamp: Timestamp,
    },
}

#[cfg(test)]
mod test {
    use super::*;
    use controller::prelude::serde_json;
    use pretty_assertions::assert_eq;

    #[test]
    fn user_private_message() {
        let json = r#"
        {
            "action": "send_message",
            "target": "00000000-0000-0000-0000-000000000000",
            "content": "Hello Bob!"
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::SendMessage { target, content } = msg {
            assert_eq!(target, Some(ParticipantId::nil()));
            assert_eq!(content, "Hello Bob!");
        } else {
            panic!()
        }
    }

    #[test]
    fn user_room_message() {
        let json = r#"
        {
            "action": "send_message",
            "content": "Hello all!"
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::SendMessage { target, content } = msg {
            assert_eq!(target, None);
            assert_eq!(content, "Hello all!");
        } else {
            panic!()
        }
    }
}
