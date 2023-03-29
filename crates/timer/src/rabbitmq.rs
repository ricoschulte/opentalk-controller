// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::{outgoing, TimerId};
use serde::{Deserialize, Serialize};
use types::core::ParticipantId;

#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
    Start(outgoing::Started),
    Stop(outgoing::Stopped),
    /// A participant updated its ready status
    UpdateReadyStatus(UpdateReadyStatus),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateReadyStatus {
    /// The timer that the update is for
    pub timer_id: TimerId,
    /// The participant that issued the update
    pub participant_id: ParticipantId,
}
