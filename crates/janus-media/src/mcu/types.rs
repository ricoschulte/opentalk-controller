// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::incoming::SubscriberConfiguration;
use anyhow::anyhow;
use controller::prelude::*;
use janus_client::{Jsep, TrickleCandidate};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use types::core::ParticipantId;

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub enum Request {
    RequestOffer { without_video: bool },
    SdpOffer(String),
    SdpAnswer(String),
    Candidate(TrickleCandidate),
    EndOfCandidates,
    PublisherConfigure(PublishConfiguration),
    SubscriberConfigure(SubscriberConfiguration),
}

#[derive(Debug)]
pub struct PublishConfiguration {
    pub video: bool,
    pub audio: bool,
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
    Media(Media),
    SlowLink(LinkDirection),
    Trickle(TrickleMessage),
    AssociatedMcuDied,
    StartedTalking,
    StoppedTalking,
}

#[derive(Debug)]
pub struct Media {
    pub kind: String,
    pub receiving: bool,
}

impl From<janus_client::incoming::Media> for Media {
    fn from(value: janus_client::incoming::Media) -> Self {
        Self {
            kind: value.kind,
            receiving: value.receiving,
        }
    }
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

impl MediaSessionType {
    pub fn as_type_str(&self) -> &'static str {
        match self {
            MediaSessionType::Video => "video",
            MediaSessionType::Screen => "screen",
        }
    }
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
