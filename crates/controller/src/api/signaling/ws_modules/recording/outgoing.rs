// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use serde::Serialize;

use super::RecordingId;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "message", rename_all = "snake_case")]
pub enum Message {
    Started(Started),
    Stopped(Stopped),
    Error(Error),
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Started {
    pub recording_id: RecordingId,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Stopped {
    pub recording_id: RecordingId,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum Error {
    InsufficientPermissions,
    AlreadyRecording,
    InvalidRecordingId,
}
