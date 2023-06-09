// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use super::RecordingId;
use serde::{Deserialize, Serialize};
use types::core::{BreakoutRoomId, RoomId};

#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    /// Signals for the recording "participant"
    Stop,

    /// Messages sent to participants to signal changes in the recording
    Started(RecordingId),
    Stopped(RecordingId),
}

/// Message sent to the recording service instructing it to record the given room
#[derive(Debug, Serialize)]
pub struct StartRecording {
    pub room: RoomId,
    pub breakout: Option<BreakoutRoomId>,
}
