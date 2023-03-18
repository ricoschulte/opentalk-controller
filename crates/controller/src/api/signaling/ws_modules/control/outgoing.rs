// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::api::signaling::Role;
use crate::api::v1::tariffs::TariffResource;
use serde::Serialize;
use std::collections::HashMap;
use types::core::{ParticipantId, Timestamp};

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
    /// The quota's time limit has elapsed
    TimeLimitQuotaElapsed,

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

    #[serde(skip_serializing_if = "Option::is_none")]
    pub closes_at: Option<Timestamp>,

    pub tariff: Box<TariffResource>,

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
    use std::str::FromStr;

    use super::*;
    use chrono::DateTime;
    use db_storage::tariffs::{Tariff, TariffId};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn participant_tariff() -> TariffResource {
        TariffResource {
            id: TariffId::nil(),
            name: "test".into(),
            quotas: Default::default(),
            enabled_modules: Default::default(),
        }
    }

    #[test]
    fn tariff_to_participant_tariff() {
        let tariff = Tariff {
            id: TariffId::nil(),
            name: "test".into(),
            created_at: Default::default(),
            updated_at: Default::default(),
            quotas: Default::default(),
            disabled_modules: vec![
                "whiteboard".to_string(),
                "timer".to_string(),
                "media".to_string(),
                "polls".to_string(),
            ],
        };
        let available_modules = vec!["chat", "media", "polls", "whiteboard", "timer"];

        let expected = json!({
            "id": "00000000-0000-0000-0000-000000000000",
            "name": "test",
            "quotas": {},
            "enabled_modules": ["chat"],
        });

        let actual =
            serde_json::to_value(TariffResource::from_tariff(tariff, &available_modules)).unwrap();

        assert_eq!(actual, expected);
    }

    #[test]
    fn join_success() {
        let expected = json!({
            "message": "join_success",
            "id": "00000000-0000-0000-0000-000000000000",
            "display_name": "name",
            "avatar_url": "http://url",
            "role": "user",
            "closes_at":"2021-06-24T14:00:11.873753715Z",
            "tariff": serde_json::to_value(participant_tariff()).unwrap(),
            "participants": [],
        });

        let produced = serde_json::to_value(&Message::JoinSuccess(JoinSuccess {
            id: ParticipantId::nil(),
            display_name: "name".into(),
            avatar_url: Some("http://url".into()),
            role: Role::User,
            closes_at: Some(
                DateTime::from_str("2021-06-24T14:00:11.873753715Z")
                    .unwrap()
                    .into(),
            ),
            tariff: participant_tariff().into(),
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
            "tariff": serde_json::to_value(participant_tariff()).unwrap(),
            "participants": [],
        });

        let produced = serde_json::to_value(&Message::JoinSuccess(JoinSuccess {
            id: ParticipantId::nil(),
            display_name: "name".into(),
            avatar_url: None,
            role: Role::Guest,
            closes_at: None,
            tariff: participant_tariff().into(),
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
