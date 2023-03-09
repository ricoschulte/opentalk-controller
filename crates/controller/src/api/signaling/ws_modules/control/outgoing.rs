// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::api::signaling::Role;
use controller_shared::ParticipantId;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "message", rename_all = "snake_case")]
pub enum Message {
    JoinSuccess(JoinSuccess),
    /// Joining the room failed
    JoinBlocked(JoinBlockedReason),
    /// State change of this participant
    Update(Participant),
    /// A participant that joined the room
    Joined(Participant),
    /// This participant left the room
    Left(AssociatedParticipant),

    RoleUpdated {
        new_role: Role,
    },

    Error(Error),
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct JoinSuccess {
    pub id: ParticipantId,

    pub display_name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,

    pub role: Role,

    pub modules: Vec<&'static str>,

    #[serde(flatten)]
    pub module_data: HashMap<&'static str, serde_json::Value>,

    pub participants: Vec<Participant>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum JoinBlockedReason {
    ParticipantLimitReached,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AssociatedParticipant {
    pub id: ParticipantId,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum Error {
    InvalidJson,
    InvalidNamespace,
    InvalidUsername,
    AlreadyJoined,
    NotYetJoined,
    NotAcceptedOrNotInWaitingRoom,
    RaiseHandsDisabled,
    InsufficientPermissions,
    TargetIsRoomOwner,
    NothingToDo,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WaitingRoomState {
    Waiting,
    Accepted,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Participant {
    pub id: ParticipantId,

    #[serde(flatten)]
    pub module_data: HashMap<&'static str, serde_json::Value>,
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn join_success() {
        let expected = json!({
            "message": "join_success",
            "id": "00000000-0000-0000-0000-000000000000",
            "display_name": "name",
            "avatar_url": "http://url",
            "role": "user",
            "modules": [],
            "participants": [],
        });

        let produced = serde_json::to_value(&Message::JoinSuccess(JoinSuccess {
            id: ParticipantId::nil(),
            role: Role::User,
            display_name: "name".into(),
            avatar_url: Some("http://url".into()),
            modules: vec![],
            module_data: Default::default(),
            participants: vec![],
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn join_success_guest() {
        let expected = json!({
            "message": "join_success",
            "id": "00000000-0000-0000-0000-000000000000",
            "display_name": "name",
            "role": "guest",
            "modules": [],
            "participants": [],
        });

        let produced = serde_json::to_value(&Message::JoinSuccess(JoinSuccess {
            id: ParticipantId::nil(),
            display_name: "name".into(),
            avatar_url: None,
            role: Role::Guest,
            modules: vec![],
            module_data: Default::default(),
            participants: vec![],
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn update() {
        let expected = json!({"message": "update", "id": "00000000-0000-0000-0000-000000000000"});

        let produced = serde_json::to_value(&Message::Update(Participant {
            id: ParticipantId::nil(),
            module_data: Default::default(),
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn joined() {
        let expected = json!({"message": "joined", "id": "00000000-0000-0000-0000-000000000000"});

        let produced = serde_json::to_value(&Message::Joined(Participant {
            id: ParticipantId::nil(),
            module_data: Default::default(),
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn left() {
        let expected = json!({"message": "left","id": "00000000-0000-0000-0000-000000000000"});

        let produced = serde_json::to_value(&Message::Left(AssociatedParticipant {
            id: ParticipantId::nil(),
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn error() {
        let expected = json!({"message": "error", "error": "raise_hands_disabled"});

        let produced = serde_json::to_value(&Message::Error(Error::RaiseHandsDisabled)).unwrap();

        assert_eq!(expected, produced);
    }
}
