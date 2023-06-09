// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::Scope;
use serde::Deserialize;
use types::core::Timestamp;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Message {
    EnableChat,
    DisableChat,
    SendMessage(SendMessage),
    ClearHistory,
    SetLastSeenTimestamp {
        #[serde(flatten)]
        scope: Scope,
        timestamp: Timestamp,
    },
}

#[derive(Debug, Deserialize)]
pub struct SendMessage {
    pub content: String,
    #[serde(flatten)]
    pub scope: Scope,
}

#[cfg(test)]
mod test {
    use super::*;
    use controller::prelude::serde_json;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use types::core::{GroupName, ParticipantId};

    #[test]
    fn user_private_message() {
        let json = json!({
            "action": "send_message",
            "scope": "private",
            "target": "00000000-0000-0000-0000-000000000000",
            "content": "Hello Bob!"
        });

        let msg: Message = serde_json::from_value(json).unwrap();

        if let Message::SendMessage(SendMessage { content, scope }) = msg {
            assert_eq!(scope, Scope::Private(ParticipantId::nil()));
            assert_eq!(content, "Hello Bob!");
        } else {
            panic!()
        }
    }

    #[test]
    fn user_group_message() {
        let json = json!({
            "action": "send_message",
            "scope": "group",
            "target": "management",
            "content": "Hello managers!"
        });

        let msg: Message = serde_json::from_value(json).unwrap();

        if let Message::SendMessage(SendMessage { content, scope }) = msg {
            assert_eq!(
                scope,
                Scope::Group(GroupName::from("management".to_owned()))
            );
            assert_eq!(content, "Hello managers!");
        } else {
            panic!()
        }
    }

    #[test]
    fn user_room_message() {
        let json = json!({
            "action": "send_message",
            "scope": "global",
            "content": "Hello all!"
        });

        let msg: Message = serde_json::from_value(json).unwrap();

        if let Message::SendMessage(SendMessage { content, scope }) = msg {
            assert_eq!(scope, Scope::Global);
            assert_eq!(content, "Hello all!");
        } else {
            panic!()
        }
    }
}
