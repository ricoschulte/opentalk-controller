use crate::api::signaling::Role;
use controller_shared::ParticipantId;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "message", rename_all = "snake_case")]
pub enum Message {
    JoinSuccess(JoinSuccess),
    /// State change of this participant
    Update(Participant),
    /// A participant that joined the room
    Joined(Participant),
    /// This participant left the room
    Left(AssociatedParticipant),

    RoleUpdated {
        new_role: Role,
    },

    Error {
        text: &'static str,
    },
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct JoinSuccess {
    pub id: ParticipantId,

    pub display_name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,

    pub role: Role,

    #[serde(flatten)]
    pub module_data: HashMap<&'static str, serde_json::Value>,

    pub participants: Vec<Participant>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct AssociatedParticipant {
    pub id: ParticipantId,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Participant {
    pub id: ParticipantId,

    #[serde(flatten)]
    pub module_data: HashMap<&'static str, serde_json::Value>,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn join_success() {
        let expected = r#"{"message":"join_success","id":"00000000-0000-0000-0000-000000000000","display_name":"name","avatar_url":"http://url","role":"user","participants":[]}"#;

        let produced = serde_json::to_string(&Message::JoinSuccess(JoinSuccess {
            id: ParticipantId::nil(),
            role: Role::User,
            display_name: "name".into(),
            avatar_url: Some("http://url".into()),
            module_data: Default::default(),
            participants: vec![],
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn join_success_guest() {
        let expected = r#"{"message":"join_success","id":"00000000-0000-0000-0000-000000000000","display_name":"name","role":"guest","participants":[]}"#;

        let produced = serde_json::to_string(&Message::JoinSuccess(JoinSuccess {
            id: ParticipantId::nil(),
            display_name: "name".into(),
            avatar_url: None,
            role: Role::Guest,
            module_data: Default::default(),
            participants: vec![],
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn update() {
        let expected = r#"{"message":"update","id":"00000000-0000-0000-0000-000000000000"}"#;

        let produced = serde_json::to_string(&Message::Update(Participant {
            id: ParticipantId::nil(),
            module_data: Default::default(),
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn joined() {
        let expected = r#"{"message":"joined","id":"00000000-0000-0000-0000-000000000000"}"#;

        let produced = serde_json::to_string(&Message::Joined(Participant {
            id: ParticipantId::nil(),
            module_data: Default::default(),
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn left() {
        let expected = r#"{"message":"left","id":"00000000-0000-0000-0000-000000000000"}"#;

        let produced = serde_json::to_string(&Message::Left(AssociatedParticipant {
            id: ParticipantId::nil(),
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn error() {
        let expected = r#"{"message":"error","text":"Error!"}"#;

        let produced = serde_json::to_string(&Message::Error { text: "Error!" }).unwrap();

        assert_eq!(expected, produced);
    }
}
