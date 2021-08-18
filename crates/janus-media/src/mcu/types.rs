use anyhow::anyhow;
use controller::prelude::*;
use janus_client::{Jsep, TrickleCandidate};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub enum Request {
    RequestOffer,
    SdpOffer(String),
    SdpAnswer(String),
    Candidate(TrickleCandidate),
    EndOfCandidates,
}

#[derive(Debug)]
pub enum Response {
    SdpAnswer(Jsep),
    SdpOffer(Jsep),
    None,
}

/// Used to relay messages to the WebSocket
#[derive(Debug)]
pub enum WebRtcEvent {
    WebRtcUp,
    WebRtcDown,
    SlowLink(LinkDirection),
    Trickle(TrickleMessage),
    AssociatedMcuDied,
}

#[derive(Debug)]
pub enum LinkDirection {
    Upstream,
    Downstream,
}

#[derive(Debug)]
pub enum TrickleMessage {
    Completed,
    Candidate(TrickleCandidate),
}

/// The type of media session
#[derive(Hash, Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum MediaSessionType {
    #[serde(rename = "video")]
    Video,
    #[serde(rename = "screen")]
    Screen,
}

impl TryFrom<u64> for MediaSessionType {
    type Error = anyhow::Error;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Video),
            2 => Ok(Self::Screen),
            _ => Err(anyhow!("Invalid media session type, {}", value)),
        }
    }
}

impl From<MediaSessionType> for u64 {
    fn from(value: MediaSessionType) -> Self {
        match value {
            MediaSessionType::Video => 1,
            MediaSessionType::Screen => 2,
        }
    }
}

impl std::fmt::Display for MediaSessionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Into::<u64>::into(*self).fmt(f)
    }
}

/// Key Type consisting of ParticipantID and MediaSessionType
///
/// Used as a key to be able to support multiple media sessions per participant.
/// Used in the description of a Janus room as fallback.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct MediaSessionKey(pub ParticipantId, pub MediaSessionType);

/// We use this mapping in the description of a Janus room
///
/// For this we need to be able to convert it into a String.
impl std::fmt::Display for MediaSessionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}|{}", self.0, self.1)
    }
}
