// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::api::signaling::prelude::*;
use serde::Serialize;
use types::core::ParticipantId;

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
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn kicked() {
        let expected = json!({"message": "kicked"});

        let produced = serde_json::to_value(&Message::Kicked).unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn banned() {
        let expected = json!({"message": "banned"});

        let produced = serde_json::to_value(&Message::Banned).unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn in_waiting_room() {
        let expected = json!({"message": "in_waiting_room"});

        let produced = serde_json::to_value(&Message::InWaitingRoom).unwrap();

        assert_eq!(expected, produced);
    }
}
