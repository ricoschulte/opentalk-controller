use crate::error;
use serde::{self, Deserialize, Serialize};
use std::{convert::TryFrom, str::FromStr};

pub mod incoming;
pub mod outgoing;

pub use incoming::{PluginData, Success};

pub trait PluginRequest: Into<outgoing::PluginBody> {
    type PluginResponse: TryFrom<incoming::PluginData>;

    /// Ignore ACK flag
    ///
    /// true for asynchronous messages to skip the ACK and wait the actual response
    /// true for sync messages which are expected to return some information
    /// false for sync messages which dont return anything and instead receive an ACK
    const IGNORE_ACK: bool = true;
}

/// Audio codecs supported by Janus
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum AudioCodec {
    #[serde(rename = "opus")]
    Opus,
    #[serde(rename = "multiopus")]
    MultiOpus,
    #[serde(rename = "isac32")]
    Isac32,
    #[serde(rename = "isac16")]
    Isac16,
    #[serde(rename = "pcmu")]
    Pcmu,
    #[serde(rename = "pcma")]
    Pcma,
    #[serde(rename = "g722")]
    G722,
}
impl FromStr for AudioCodec {
    type Err = crate::error::Error;
    fn from_str(value: &str) -> Result<Self, crate::error::Error> {
        Ok(match value {
            "opus" => Self::Opus,
            "multiopus" => Self::MultiOpus,
            "isac32" => Self::Isac32,
            "isac16" => Self::Isac16,
            "pcmu" => Self::Pcmu,
            "pcma" => Self::Pcma,
            "g722" => Self::G722,
            _ => return Err(crate::error::Error::UnknownAudioCodec(value.to_owned())),
        })
    }
}

/// Video codecs supported by Janus
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum VideoCodec {
    #[serde(rename = "vp8")]
    Vp8,
    #[serde(rename = "vp9")]
    Vp9,
    #[serde(rename = "h264")]
    H264,
    #[serde(rename = "av1")]
    Av1,
    #[serde(rename = "h265")]
    H265,
}

impl FromStr for VideoCodec {
    type Err = crate::error::Error;
    fn from_str(value: &str) -> Result<Self, crate::error::Error> {
        Ok(match value {
            "vp8" => Self::Vp8,
            "vp9" => Self::Vp9,
            "h264" => Self::H265,
            "av1" => Self::Av1,
            "h265" => Self::H265,
            _ => return Err(crate::error::Error::UnknownVideoCodec(value.to_owned())),
        })
    }
}

fn is_default<T: Default + PartialEq>(t: &T) -> bool {
    t == &T::default()
}

/// JanusPlugin
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum JanusPlugin {
    #[cfg(feature = "videoroom")]
    #[serde(rename = "janus.plugin.videoroom")]
    VideoRoom,
    #[cfg(feature = "echotest")]
    #[serde(rename = "janus.plugin.echotest")]
    Echotest,
}

/// A Janus API session identifier
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(u64);
impl SessionId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }
}

impl From<u64> for SessionId {
    fn from(val: u64) -> Self {
        Self::new(val)
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// A identifer resembling as Janus API session to a specific plugin
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HandleId(u64);
impl HandleId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }
}

impl From<u64> for HandleId {
    fn from(val: u64) -> Self {
        Self::new(val)
    }
}
impl std::fmt::Display for HandleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// A Room identifier
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RoomId(u64);
impl RoomId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }
}

impl From<u64> for RoomId {
    fn from(val: u64) -> Self {
        Self::new(val)
    }
}

impl From<RoomId> for u64 {
    fn from(value: RoomId) -> Self {
        value.0
    }
}
impl std::fmt::Display for RoomId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// A Feed identifier
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FeedId(u64);
impl FeedId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }
}

impl From<u64> for FeedId {
    fn from(val: u64) -> Self {
        Self::new(val)
    }
}

impl From<FeedId> for u64 {
    fn from(value: FeedId) -> Self {
        value.0
    }
}

/// A transaction identifier
///
/// Used to match an async request to Janus to the response
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub(crate) struct TransactionId(String);
impl TransactionId {
    pub fn new(value: String) -> Self {
        Self(value)
    }
}

impl From<u64> for TransactionId {
    fn from(val: u64) -> Self {
        Self::new(val.to_string())
    }
}

/// The streamtypes we utilize
// todo move to k3k-signaling
pub enum StreamTypes {
    Video,
    Screen,
}
impl From<u64> for StreamTypes {
    fn from(value: u64) -> Self {
        match value {
            1 => Self::Video,
            2 => Self::Screen,
            _ => unimplemented!(),
        }
    }
}

/// A candidate for ICE/SDP trickle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrickleCandidate {
    // Todo It can be Either sdpMid or sdpMLineIndex. Making both mandatory for now.
    // We also can find beter field names for this. sdp_medium_id or something?
    #[serde(rename = "sdpMid")]
    pub sdp_m_id: String,
    #[serde(rename = "sdpMLineIndex")]
    pub sdp_m_line_index: u64,
    pub candidate: String,
}

/// The type of the SDP in the JSEP
// todo There might be more, eg. requestoffer
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum JsepType {
    #[serde(rename = "offer")]
    Offer,
    #[serde(rename = "answer")]
    Answer,
}

/// A JavaScript Session Establishment Protocol struct Janus expects
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jsep {
    #[serde(rename = "type")]
    kind: JsepType,
    sdp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    trickle: Option<bool>,
}
impl Jsep {
    /// Returns the type of this JSEP
    pub fn kind(&self) -> JsepType {
        self.kind
    }

    /// Returns the sdp message. Consumes the JSEP
    pub fn sdp(self) -> String {
        self.sdp
    }
}

/// A SDP offer
#[derive(Debug, Serialize, Deserialize)]
pub struct SdpOffer(Jsep);

impl From<(JsepType, String)> for SdpOffer {
    fn from(value: (JsepType, String)) -> Self {
        Self(Jsep {
            kind: value.0,
            sdp: value.1,
            trickle: None,
        })
    }
}
impl From<SdpOffer> for Jsep {
    fn from(value: SdpOffer) -> Self {
        value.0
    }
}

/// A SDP answer
#[derive(Debug, Serialize, Deserialize)]
pub struct SdpAnswer(Jsep);

impl From<(JsepType, String)> for SdpAnswer {
    fn from(value: (JsepType, String)) -> Self {
        Self(Jsep {
            kind: value.0,
            sdp: value.1,
            trickle: None,
        })
    }
}
impl From<SdpAnswer> for Jsep {
    fn from(value: SdpAnswer) -> Self {
        value.0
    }
}
impl TryFrom<Jsep> for SdpAnswer {
    type Error = error::Error;

    fn try_from(value: Jsep) -> Result<Self, Self::Error> {
        if matches!(value.kind, JsepType::Answer) {
            Ok(SdpAnswer(value))
        } else {
            Err(error::Error::InvalidConversion(format!(
                "TryFrom Jsep {:?} into SdpAnswer",
                value
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use outgoing::{AttachToPlugin, CreateSession, JanusRequest, KeepAlive};
    use pretty_assertions::assert_eq;

    #[test]
    fn create() {
        assert_eq!(
            serde_json::json!({
                "janus": "create",
                "transaction": "k3k-rulez"
            })
            .to_string(),
            serde_json::to_string(&JanusRequest::CreateSession(CreateSession {
                transaction: TransactionId::new("k3k-rulez".into())
            }))
            .unwrap()
        );
    }

    #[test]
    fn attach() {
        assert_eq!(
            serde_json::json!({
                "janus": "attach",
                "plugin": JanusPlugin::VideoRoom,
                "transaction": "k3k-rulez",
                "session_id": 123
            })
            .to_string(),
            serde_json::to_string(&JanusRequest::AttachToPlugin(AttachToPlugin {
                transaction: TransactionId::new("k3k-rulez".into()),
                plugin: JanusPlugin::VideoRoom,
                session_id: SessionId::new(123)
            }))
            .unwrap()
        );
    }
    #[test]
    fn keepalive() {
        assert_eq!(
            serde_json::json!({
                "janus": "keepalive",
                "session_id": 134,
                "transaction": "k3k-rulez"
            })
            .to_string(),
            serde_json::to_string(&JanusRequest::KeepAlive(KeepAlive {
                session_id: SessionId::new(134),
                transaction: TransactionId::new("k3k-rulez".into())
            }))
            .unwrap()
        );
    }
}
