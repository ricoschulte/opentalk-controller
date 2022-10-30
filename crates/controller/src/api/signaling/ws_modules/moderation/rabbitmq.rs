// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use controller_shared::ParticipantId;
use serde::{Deserialize, Serialize};

/// Control messages sent between controller modules to communicate changes inside a room
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Message {
    Kicked(ParticipantId),
    Banned(ParticipantId),
    JoinedWaitingRoom(ParticipantId),
    LeftWaitingRoom(ParticipantId),
    WaitingRoomEnableUpdated,
}
