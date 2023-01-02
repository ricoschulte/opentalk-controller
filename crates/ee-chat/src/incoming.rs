use serde::Deserialize;

/// Message received from websocket
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Message {
    SendMessage { group: String, content: String },
}

#[cfg(test)]
mod test {
    use super::*;
    use controller::prelude::serde_json::{self, json};
    use pretty_assertions::assert_eq;

    #[test]
    fn user_group_message() {
        let json = json!(
        {
            "action": "send_message",
            "group": "management",
            "content": "Hello managers!"
        }
        );

        let msg: Message = serde_json::from_value(json).unwrap();

        let Message::SendMessage { group, content } = msg;
        assert_eq!(group, "management".to_string());
        assert_eq!(content, "Hello managers!");
    }
}
