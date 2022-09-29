use crate::api::signaling::prelude::*;
use controller_shared::ParticipantId;
use serde::Serialize;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "message", rename_all = "snake_case")]
pub enum Message {
    Kicked,
    Banned,

    InWaitingRoom,

    JoinedWaitingRoom(control::outgoing::Participant),
    LeftWaitingRoom(control::outgoing::AssociatedParticipant),

    WaitingRoomEnabled,
    WaitingRoomDisabled,

    RaiseHandsEnabled { issued_by: ParticipantId },
    RaiseHandsDisabled { issued_by: ParticipantId },

    Accepted,

    Error(Error),

    RaisedHandResetByModerator { issued_by: ParticipantId },
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum Error {
    CannotBanGuest,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn kicked() {
        let expected = r#"{"message":"kicked"}"#;

        let produced = serde_json::to_string(&Message::Kicked).unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn banned() {
        let expected = r#"{"message":"banned"}"#;

        let produced = serde_json::to_string(&Message::Banned).unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn in_waiting_room() {
        let expected = r#"{"message":"in_waiting_room"}"#;

        let produced = serde_json::to_string(&Message::InWaitingRoom).unwrap();

        assert_eq!(expected, produced);
    }
}
