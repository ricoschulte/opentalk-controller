// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use serde::{Deserialize, Serialize};
use types::core::ParticipantId;

use crate::incoming::ParticipantSelection;

#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    StartedTalking(ParticipantId),
    StoppedTalking(ParticipantId),
    RequestMute(RequestMute),
    PresenterGranted(ParticipantSelection),
    PresenterRevoked(ParticipantSelection),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestMute {
    /// The issuer of the mute request
    pub issuer: ParticipantId,
    /// Flag to determine if the mute shall be forced or not
    pub force: bool,
}
