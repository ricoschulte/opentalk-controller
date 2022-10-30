// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use super::{AssocParticipantInOtherRoom, BreakoutRoom, BreakoutRoomId, ParticipantInOtherRoom};
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "message", rename_all = "snake_case")]
pub enum Message {
    Started(Started),
    Stopped,
    Expired,

    Joined(ParticipantInOtherRoom),
    Left(AssocParticipantInOtherRoom),

    Error(Error),
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Started {
    pub rooms: Vec<BreakoutRoom>,
    pub expires: Option<DateTime<Utc>>,
    pub assignment: Option<BreakoutRoomId>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum Error {
    Inactive,
    InsufficientPermissions,
}

#[cfg(test)]
mod test {
    use crate::api::signaling::{prelude::control::ParticipationKind, Role, Timestamp};

    use super::*;
    use controller_shared::ParticipantId;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use test_util::assert_eq_json;
    use uuid::Uuid;

    #[test]
    fn started() {
        let expected = json!({
            "message": "started",
            "rooms": [
                {"id":"00000000-0000-0000-0000-000000000000", "name":"Room 1"},
                {"id":"00000000-0000-0000-0000-000000000001","name":"Room 2"},
            ],
            "expires": null,
            "assignment": "00000000-0000-0000-0000-000000000000",
        });

        let produced = serde_json::to_value(&Message::Started(Started {
            rooms: vec![
                BreakoutRoom {
                    id: BreakoutRoomId(Uuid::from_u128(0)),
                    name: "Room 1".into(),
                },
                BreakoutRoom {
                    id: BreakoutRoomId(Uuid::from_u128(1)),
                    name: "Room 2".into(),
                },
            ],
            expires: None,
            assignment: Some(BreakoutRoomId::nil()),
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn stopped() {
        let expected = json!({"message": "stopped"});

        let produced = serde_json::to_value(&Message::Stopped).unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn expired() {
        let expected = json!({"message": "expired"});

        let produced = serde_json::to_value(&Message::Expired).unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn joined() {
        assert_eq_json!(Message::Joined(ParticipantInOtherRoom {
            breakout_room: Some(BreakoutRoomId::nil()),
            id: ParticipantId::nil(),
            display_name: "test".into(),
            role: Role::Moderator,
            avatar_url: Some("example.org/avatar.png".into()),
            participation_kind: ParticipationKind::User,
            joined_at: Timestamp::unix_epoch(),
            left_at: None,
        }), {
            "message": "joined",
            "breakout_room": "00000000-0000-0000-0000-000000000000",
            "id": "00000000-0000-0000-0000-000000000000",
            "display_name": "test",
            "role": "moderator",
            "avatar_url": "example.org/avatar.png",
            "participation_kind": "user",
            "joined_at": "1970-01-01T00:00:00Z",
            "left_at": ()
        });
    }

    #[test]
    fn left() {
        let expected = json!({
            "message": "left",
            "breakout_room": "00000000-0000-0000-0000-000000000000",
            "id": "00000000-0000-0000-0000-000000000000",
        });

        let produced = serde_json::to_value(&Message::Left(AssocParticipantInOtherRoom {
            breakout_room: Some(BreakoutRoomId::nil()),
            id: ParticipantId::nil(),
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn error() {
        let expected = json!({"message": "error", "error": "insufficient_permissions"});

        let produced =
            serde_json::to_value(&Message::Error(Error::InsufficientPermissions)).unwrap();

        assert_eq!(expected, produced);
    }
}
