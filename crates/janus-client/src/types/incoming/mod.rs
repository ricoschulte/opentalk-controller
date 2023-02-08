// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! Incoming Datatypes
//!
//! This are the response types sent async from Janus via the websocket.

use super::{AudioCodec, TrickleCandidate, VideoCodec};
use crate::error::JanusError;
use crate::{error, types::Jsep, types::TransactionId, HandleId, SessionId};
use serde::{self, Deserialize};
use std::convert::TryFrom;

#[cfg(feature = "echotest")]
pub use echotest::{EchoPluginData, EchoPluginDataEvent, EchoPluginUnnamed};

#[cfg(feature = "videoroom")]
pub use videoroom::{
    VideoRoomPluginData, VideoRoomPluginDataAttached, VideoRoomPluginDataCreated,
    VideoRoomPluginDataDestroyed, VideoRoomPluginDataJoined, VideoRoomPluginDataStarted,
    VideoRoomPluginDataSuccess, VideoRoomPluginEvent, VideoRoomPluginEventConfigured,
    VideoRoomPluginEventLeaving, VideoRoomPluginEventStarted,
};

#[cfg(feature = "echotest")]
mod echotest;
#[cfg(feature = "videoroom")]
mod videoroom;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "janus")]
pub enum JanusMessage {
    #[serde(rename = "ack")]
    Ack(Ack),
    #[serde(rename = "keepalive")]
    KeepAlive(KeepAlive),
    #[serde(rename = "event")]
    Event(Event),
    /// Response of type success
    #[serde(rename = "success")]
    Success(Success),
    #[serde(rename = "timeout")]
    Timeout(Timeout),
    #[serde(rename = "error")]
    Error(Error),
    #[serde(rename = "hangup")]
    Hangup(Hangup),
    #[serde(rename = "trickle")]
    Trickle(TrickleMessage),
    #[serde(rename = "webrtcup")]
    WebRtcUp(WebRtcUp),
    #[serde(rename = "media")]
    Media(Media),
    #[serde(rename = "detached")]
    Detached(Detached),
    #[serde(rename = "slowlink")]
    SlowLink(SlowLink),
}

impl TryFrom<JanusMessage> for (PluginData, Option<Jsep>) {
    type Error = error::Error;

    fn try_from(value: JanusMessage) -> Result<Self, Self::Error> {
        match value {
            JanusMessage::Event(e) => Ok((e.plugindata, e.jsep)),
            JanusMessage::Success(Success::Plugin(e)) => Ok((e.plugindata, e.jsep)),
            _ => Err(error::Error::InvalidConversion(format!(
                "TryFrom JanusMessage {value:?} into (PluginData, Option<Jsep>)",
            ))),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ack {
    pub(crate) transaction: TransactionId,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KeepAlive {
    pub session_id: SessionId,
    pub(crate) transaction: TransactionId,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Event {
    pub sender: HandleId,
    pub session_id: SessionId,
    #[serde(default)]
    pub(crate) transaction: Option<TransactionId>,
    pub plugindata: PluginData,
    pub jsep: Option<Jsep>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Error {
    pub session_id: SessionId,
    pub(crate) transaction: Option<TransactionId>,
    pub error: JanusError,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Timeout {
    pub session_id: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Hangup {
    pub session_id: SessionId,
    pub sender: HandleId,
    pub reason: String,
}
#[derive(Debug, Clone, Deserialize)]
pub struct TrickleMessage {
    pub session_id: SessionId,
    pub sender: HandleId,
    pub candidate: TrickleInnerMessage,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum TrickleInnerMessage {
    Completed { completed: bool },
    Candidate(TrickleCandidate),
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebRtcUp {
    pub session_id: SessionId,
    pub sender: HandleId,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Media {
    pub session_id: SessionId,
    pub sender: HandleId,
    #[serde(rename = "type")]
    pub kind: String,
    pub receiving: bool,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Detached {
    pub session_id: SessionId,
    pub sender: HandleId,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SlowLink {
    pub session_id: SessionId,
    pub sender: HandleId,
    pub media: String,
    pub uplink: bool,
    pub lost: u64,
}

impl JanusMessage {
    pub(crate) fn transaction_id(&self) -> Option<&TransactionId> {
        match self {
            JanusMessage::KeepAlive(KeepAlive { transaction, .. }) => Some(transaction),
            JanusMessage::Success(e) => Some(e.transaction_id()),
            JanusMessage::Ack(Ack { transaction, .. }) => Some(transaction),
            JanusMessage::Event(Event { transaction, .. }) => transaction.as_ref(),
            JanusMessage::Error(Error { transaction, .. }) => transaction.as_ref(),
            JanusMessage::Timeout(_) => None,
            JanusMessage::Hangup(_) => None,
            JanusMessage::Trickle(_) => None,
            JanusMessage::WebRtcUp(_) => None,
            JanusMessage::Media(_) => None,
            JanusMessage::Detached(_) => None,
            JanusMessage::SlowLink(_) => None,
        }
    }

    /// Convert the message into a error if the message is an error
    pub(crate) fn into_result(self) -> Result<Self, error::Error> {
        match self {
            JanusMessage::Error(e) => Err(error::Error::JanusError(e.error)),
            JanusMessage::Event(Event {
                plugindata:
                    PluginData::EchoTest(EchoPluginData::Event(EchoPluginDataEvent::Error(error))),
                ..
            })
            | JanusMessage::Event(Event {
                plugindata:
                    PluginData::VideoRoom(VideoRoomPluginData::Event(VideoRoomPluginEvent::Error(
                        error,
                    ))),
                ..
            }) => Err(error::Error::JanusPluginError(error)),
            msg => Ok(msg),
        }
    }
}

/// Success response type
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Success {
    // Order of the enum is important to parsing since
    // JanusSuccess will always match all PluginSuccess messages
    Plugin(PluginSuccess),
    Janus(JanusSuccess),
}

impl Success {
    pub(crate) fn transaction_id(&self) -> &TransactionId {
        match self {
            Self::Plugin(PluginSuccess { transaction, .. }) => transaction,
            Self::Janus(JanusSuccess { transaction, .. }) => transaction,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct JanusSuccess {
    pub sender: Option<HandleId>,
    pub session_id: Option<SessionId>,
    pub(crate) transaction: TransactionId,
    pub data: Option<SuccessJanus>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginSuccess {
    pub sender: Option<HandleId>,
    pub session_id: Option<SessionId>,
    pub(crate) transaction: TransactionId,
    pub plugindata: PluginData,
    pub jsep: Option<Jsep>,
}

/// Success from Janus (ICE, DTLS, etc.)
#[derive(Debug, Clone, Deserialize)]
pub struct SuccessJanus {
    pub(crate) id: u64,
}

/// Plugin result
///
/// Each supported plugin has its variant
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "plugin", content = "data")]
pub enum PluginData {
    #[cfg(feature = "videoroom")]
    #[serde(rename = "janus.plugin.videoroom")]
    VideoRoom(VideoRoomPluginData),
    #[cfg(feature = "echotest")]
    #[serde(rename = "janus.plugin.echotest")]
    EchoTest(EchoPluginData),
}
