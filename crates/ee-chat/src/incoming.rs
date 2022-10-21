use controller::prelude::*;
use serde::Deserialize;

/// Message received from websocket
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Message {
    SendMessage { group: String, content: String },
    SetLastSeenTimestamp { group: String, timestamp: Timestamp },
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::{DateTime, Utc};
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

        if let Message::SendMessage { group, content } = msg {
            assert_eq!(group, "management".to_string());
            assert_eq!(content, "Hello managers!");
        }
    }

    #[test]
    fn set_last_seen_timestamp() {
        let json = json!(
        {
            "action": "set_last_seen_timestamp",
            "group": "management",
            "timestamp": "2022-01-01T10:11:12+02:00"
        }
        );

        let msg: Message = serde_json::from_value(json).unwrap();

        if let Message::SetLastSeenTimestamp { group, timestamp } = msg {
            assert_eq!(group, "management".to_string());
            assert_eq!(
                timestamp,
                DateTime::<Utc>::from(
                    DateTime::parse_from_rfc3339("2022-01-01T10:11:12+02:00").unwrap()
                )
                .into()
            );
        }
    }
}
