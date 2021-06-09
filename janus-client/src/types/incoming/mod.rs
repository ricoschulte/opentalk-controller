//! Incoming Datatypes
//!
//! This are the response types sent async from Janus via the websocket.

use super::{AudioCodec, TrickleCandidate, VideoCodec};
use crate::error::{JanusError, JanusInternalError};
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
    WebRtcUpdate(WebRtcUpdate),
    #[serde(rename = "media")]
    Media(Media),
    #[serde(rename = "detached")]
    Detached,
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
                "TryFrom JanusMessage {:?} into (PluginData, Option<Jsep>)",
                value
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
    pub candidate: TrickleCandidate,
    pub completed: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebRtcUpdate {
    pub session_id: SessionId,
    pub sender: HandleId,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Media {
    #[serde(rename = "type")]
    pub kind: String,
    pub receiving: bool,
}
#[derive(Debug, Clone, Deserialize)]
pub struct SlowLink {
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
            JanusMessage::WebRtcUpdate(_) => None,
            JanusMessage::Media(_) => None,
            JanusMessage::Detached => None,
            JanusMessage::SlowLink(_) => None,
        }
    }
}

/// Success response type
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Success {
    Janus(JanusSuccess),
    Plugin(PluginSuccess),
}

impl Success {
    pub(crate) fn transaction_id(&self) -> &TransactionId {
        match self {
            Self::Janus(JanusSuccess { transaction, .. }) => transaction,
            Self::Plugin(PluginSuccess { transaction, .. }) => transaction,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct JanusSuccess {
    pub sender: Option<HandleId>,
    pub session_id: Option<SessionId>,
    pub(crate) transaction: TransactionId,
    pub data: SuccessJanus,
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
