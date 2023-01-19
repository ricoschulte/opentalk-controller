// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use displaydoc::Display;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use thiserror::Error;

#[derive(Debug, Display, Error)]
pub enum Error {
    /// RabbitMQ Error: {0}
    RabbitMq(#[from] lapin::Error),
    /// WebSocket error {0}
    Websocket(#[from] tokio_tungstenite::tungstenite::Error),
    /// Not connected to Janus
    NotConnected,
    /// Failed to create a session
    FailedToCreateSession,
    /// Got invalid response from Janus
    InvalidResponse,
    /// Got invalid response from Janus {0}
    InvalidJsonResponse(#[from] serde_json::Error),
    /// Tried to get invalid session
    InvalidSession,
    /// Unkown audio codec
    UnknownAudioCodec(String),
    /// Unkown video codec
    UnknownVideoCodec(String),
    /// Invalid or no candidate
    InvalidCandidates,
    /// Janus error {0}
    JanusError(#[from] JanusError),
    /// Janus plugin error {0}
    JanusPluginError(#[from] JanusPluginError),
    /// Invalid conversion {0}
    InvalidConversion(String),
    /// Timeout
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize, Error)]
#[error("{reason}")]
pub struct JanusError {
    code: JanusInternalError,
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Error)]
#[error("{error}")]
pub struct JanusPluginError {
    error: String,
    error_code: JanusInternalError,
}

impl JanusError {
    /// Returns the error code of the JanusError
    pub fn error_code(&self) -> JanusInternalError {
        self.code
    }

    /// Returns the reason for this error
    pub fn reason(&self) -> &String {
        &self.reason
    }
}

impl JanusPluginError {
    /// Returns the error code of the JanusError
    pub fn error_code(&self) -> JanusInternalError {
        self.error_code
    }

    /// Returns the reason for this error
    pub fn reason(&self) -> &String {
        &self.error
    }
}

#[derive(Serialize_repr, Deserialize_repr, PartialEq, Eq, Debug, Clone, Copy)]
#[repr(u32)]
pub enum JanusInternalError {
    // JanusOk = 0,
    ErrorUnknown = 490,

    ErrorUnauthorized = 403,
    ErrorUnauthorizedPlugin = 405,

    ErrorTransportSpecific = 450,
    ErrorMissingRequest = 452,
    ErrorUnknownRequest = 453,
    ErrorInvalidJson = 454,
    ErrorInvalidJsonObject = 455,
    ErrorMissingMandatoryElement = 456,
    ErrorInvalidRequestPath = 457,
    ErrorSessionNotFound = 458,
    ErrorHandleNotFound = 459,
    ErrorPluginNotFound = 460,
    ErrorPluginAttach = 461,
    ErrorPluginMessage = 462,
    ErrorPluginDetach = 463,
    ErrorJsepUnknownType = 464,
    ErrorJsepInvalidSdp = 465,
    ErrorTrickeInvalidStream = 466,
    ErrorInvalidElementType = 467,
    ErrorSessionConflict = 468,
    ErrorUnexpectedAnswer = 469,
    ErrorTokenNotFound = 470,

    VideoroomErrorUnknownError = 499,

    EchotestErrorNoMessage = 411,
    EchotestErrorInvalidJson = 412,
    EchotestErrorInvalidElement = 413,
    EchotestErrorInvalidSdp = 414,

    VideoroomErrorNoMessage = 421,
    VideoroomErrorInvalidJson = 422,
    VideoroomErrorInvalidRequest = 423,
    VideoroomErrorJoinFirst = 424,
    VideoroomErrorAlreadyJoined = 425,
    VideoroomErrorNoSuchRoom = 426,
    VideoroomErrorRoomExists = 427,
    VideoroomErrorNoSuchFeed = 428,
    VideoroomErrorMissingElement = 429,
    VideoroomErrorInvalidElement = 430,
    VideoroomErrorInvalidSdpType = 431,
    VideoroomErrorPublishersFull = 432,
    VideoroomErrorUnauthorized = 433,
    VideoroomErrorAlreadyPublished = 434,
    VideoroomErrorNotPublished = 435,
    VideoroomErrorIdExists = 436,
    VideoroomErrorInvalidSdp = 437,
}
