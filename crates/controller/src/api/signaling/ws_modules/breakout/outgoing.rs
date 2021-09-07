use super::{AssocParticipantInOtherRoom, BreakoutRoom, BreakoutRoomId, ParticipantInOtherRoom};
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(tag = "message", rename_all = "snake_case")]
pub enum Message {
    Started(Started),
    Stopped,
    Expired,

    Joined(ParticipantInOtherRoom),
    Left(AssocParticipantInOtherRoom),

    Error(Error),
}

#[derive(Debug, Serialize)]
pub struct Started {
    pub rooms: Vec<BreakoutRoom>,
    pub expires: Option<DateTime<Utc>>,
    pub assignment: Option<BreakoutRoomId>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum Error {
    InvalidAssignment,
    InsufficientPermissions,
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::api::signaling::ParticipantId;

    #[test]
    fn started() {
        let expected =
            r#"{"message":"started","rooms":[{"id":0},{"id":1}],"expires":null,"assignment":0}"#;

        let produced = serde_json::to_string(&Message::Started(Started {
            rooms: vec![
                BreakoutRoom {
                    id: BreakoutRoomId(0),
                },
                BreakoutRoom {
                    id: BreakoutRoomId(1),
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
        let expected = r#"{"message":"stopped"}"#;

        let produced = serde_json::to_string(&Message::Stopped).unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn expired() {
        let expected = r#"{"message":"expired"}"#;

        let produced = serde_json::to_string(&Message::Expired).unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn joined() {
        let expected = r#"{"message":"joined","breakout_room":0,"id":"00000000-0000-0000-0000-000000000000","display_name":"test"}"#;

        let produced = serde_json::to_string(&Message::Joined(ParticipantInOtherRoom {
            breakout_room: Some(BreakoutRoomId::nil()),
            id: ParticipantId::nil(),
            display_name: "test".into(),
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn left() {
        let expected =
            r#"{"message":"left","breakout_room":0,"id":"00000000-0000-0000-0000-000000000000"}"#;

        let produced = serde_json::to_string(&Message::Left(AssocParticipantInOtherRoom {
            breakout_room: Some(BreakoutRoomId::nil()),
            id: ParticipantId::nil(),
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn error() {
        let expected = r#"{"message":"error","error":"insufficient_permissions"}"#;

        let produced =
            serde_json::to_string(&Message::Error(Error::InsufficientPermissions)).unwrap();

        assert_eq!(expected, produced);
    }
}
