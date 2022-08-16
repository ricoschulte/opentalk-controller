use crate::mcu::{self, MediaSessionKey, MediaSessionType};
use crate::rabbitmq;
use controller_shared::ParticipantId;
use janus_client::TrickleCandidate;
use serde::Serialize;

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "message")]
pub enum Message {
    /// SDP Offer, renegotiate publish
    #[serde(rename = "sdp_offer")]
    SdpOffer(Sdp),
    /// SDP Answer, start the publish/subscription
    #[serde(rename = "sdp_answer")]
    SdpAnswer(Sdp),
    /// SDP Candidate, used for ICE negotiation
    #[serde(rename = "sdp_candidate")]
    SdpCandidate(SdpCandidate),

    /// SDP End of Candidate, used for ICE negotiation
    #[serde(rename = "sdp_end_of_candidates")]
    SdpEndCandidates(Source),

    /// Signals that a webrtc connection has been established
    #[serde(rename = "webrtc_up")]
    WebRtcUp(Source),

    /// Signals that a webrtc connection has been disconnected/destryoed by janus
    ///
    /// This message can, but wont always be received when a participant disconnects
    #[serde(rename = "webrtc_down")]
    WebRtcDown(Source),

    /// Signals the media status for a participant
    #[serde(rename = "media_status")]
    Media(Media),

    /// A webrtc connection experienced package loss
    #[serde(rename = "webrtc_slow")]
    WebRtcSlow(Link),

    #[serde(rename = "focus_update")]
    FocusUpdate(FocusUpdate),

    #[serde(rename = "request_mute")]
    RequestMute(rabbitmq::RequestMute),

    /// Contains a error about what request failed. See [`Error`]
    #[serde(rename = "error")]
    Error(Error),
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Sdp {
    /// The payload of the sdp message
    pub sdp: String,

    #[serde(flatten)]
    pub source: Source,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SdpCandidate {
    /// The payload of the sdp message
    pub candidate: TrickleCandidate,

    #[serde(flatten)]
    pub source: Source,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Source {
    /// The source of this message
    pub source: ParticipantId,

    /// The type of stream
    pub media_session_type: MediaSessionType,
}

impl From<MediaSessionKey> for Source {
    fn from(media_session_key: MediaSessionKey) -> Self {
        Self {
            source: media_session_key.0,
            media_session_type: media_session_key.1,
        }
    }
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Media {
    #[serde(flatten)]
    pub source: Source,
    pub kind: String,
    pub receiving: bool,
}

impl From<(MediaSessionKey, mcu::Media)> for Media {
    fn from(value: (MediaSessionKey, mcu::Media)) -> Self {
        Self {
            source: value.0.into(),
            kind: value.1.kind,
            receiving: value.1.receiving,
        }
    }
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LinkDirection {
    Upstream,
    Downstream,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Link {
    pub direction: LinkDirection,
    #[serde(flatten)]
    pub source: Source,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct FocusUpdate {
    pub focus: Option<ParticipantId>,
}

/// Represents a error of the janus media module
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "error")]
pub enum Error {
    InvalidSdpOffer,
    HandleSdpAnswer,
    InvalidCandidate,
    InvalidEndOfCandidates,
    InvalidRequestOffer,
    InvalidConfigureRequest,
    PermissionDenied,
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::rabbitmq::RequestMute;
    use controller::prelude::*;
    use test_util::assert_eq_json;

    #[test]
    fn sdp_offer() {
        let sdp_offer = Message::SdpOffer(Sdp {
            sdp: "v=0...".into(),
            source: Source {
                source: ParticipantId::nil(),
                media_session_type: MediaSessionType::Video,
            },
        });

        assert_eq_json!(
            sdp_offer,
            {
                "message": "sdp_offer",
                "sdp": "v=0...",
                "source": "00000000-0000-0000-0000-000000000000",
                "media_session_type": "video"
            }
        );
    }

    #[test]
    fn sdp_answer() {
        let sdp_answer = Message::SdpAnswer(Sdp {
            sdp: "v=0...".into(),
            source: Source {
                source: ParticipantId::nil(),
                media_session_type: MediaSessionType::Video,
            },
        });

        assert_eq_json!(
            sdp_answer,
            {
                "message": "sdp_answer",
                "sdp": "v=0...",
                "source": "00000000-0000-0000-0000-000000000000",
                "media_session_type": "video"
            }
        );
    }

    #[test]
    fn sdp_candidate() {
        let sdp_candidate = Message::SdpCandidate(SdpCandidate {
            candidate: TrickleCandidate {
                sdp_m_line_index: 1,
                candidate: "candidate:4 1 UDP 123456 192.168.178.1 123456 typ host".into(),
            },
            source: Source {
                source: ParticipantId::nil(),
                media_session_type: MediaSessionType::Video,
            },
        });

        assert_eq_json!(
            sdp_candidate,
            {
                "message": "sdp_candidate",
                "candidate": {
                    "sdpMLineIndex": 1,
                    "candidate": "candidate:4 1 UDP 123456 192.168.178.1 123456 typ host"
                },
                "source": "00000000-0000-0000-0000-000000000000",
                "media_session_type": "video"
              }
        );
    }

    #[test]
    fn test_webrtc_up() {
        let webrtc_up = Message::WebRtcUp(Source {
            source: ParticipantId::nil(),
            media_session_type: MediaSessionType::Video,
        });

        assert_eq_json!(
            webrtc_up,
            {
                "message": "webrtc_up",
                "source": "00000000-0000-0000-0000-000000000000",
                "media_session_type": "video"
            }
        );
    }

    #[test]
    fn test_webrtc_down() {
        let webrtc_down = Message::WebRtcDown(Source {
            source: ParticipantId::nil(),
            media_session_type: MediaSessionType::Video,
        });

        assert_eq_json!(
            webrtc_down,
            {
                "message": "webrtc_down",
                "source": "00000000-0000-0000-0000-000000000000",
                "media_session_type": "video"
            }
        );
    }

    #[test]
    fn test_media_status() {
        let webrtc_down = Message::Media(Media {
            source: Source {
                source: ParticipantId::nil(),
                media_session_type: MediaSessionType::Video,
            },
            kind: "video".to_owned(),
            receiving: true,
        });

        assert_eq_json!(
            webrtc_down,
            {
                "message": "media_status",
                "source": "00000000-0000-0000-0000-000000000000",
                "media_session_type": "video",
                "kind": "video",
                "receiving": true
            }
        );
    }

    #[test]
    fn test_webrtc_slow() {
        let web_rtc_slow = Message::WebRtcSlow(Link {
            direction: LinkDirection::Upstream,
            source: Source {
                source: ParticipantId::nil(),
                media_session_type: MediaSessionType::Video,
            },
        });

        assert_eq_json!(
            web_rtc_slow,
            {
                "message": "webrtc_slow",
                "direction": "upstream",
                "source": "00000000-0000-0000-0000-000000000000",
                "media_session_type": "video"
            }
        );
    }

    #[test]
    fn test_request_mute() {
        let request_mute = Message::RequestMute(RequestMute {
            issuer: ParticipantId::nil(),
            force: false,
        });

        assert_eq_json!(
            request_mute,
            {
                "message": "request_mute",
                "issuer": "00000000-0000-0000-0000-000000000000",
                "force": false
            }
        );
    }

    #[test]
    fn test_errors() {
        let errors_and_expected = vec![
            (Error::InvalidSdpOffer, "{\"error\":\"invalid_sdp_offer\"}"),
            (Error::HandleSdpAnswer, "{\"error\":\"handle_sdp_answer\"}"),
            (Error::InvalidCandidate, "{\"error\":\"invalid_candidate\"}"),
            (
                Error::InvalidEndOfCandidates,
                "{\"error\":\"invalid_end_of_candidates\"}",
            ),
            (
                Error::InvalidRequestOffer,
                "{\"error\":\"invalid_request_offer\"}",
            ),
            (
                Error::InvalidConfigureRequest,
                "{\"error\":\"invalid_configure_request\"}",
            ),
        ];

        for (error, expected) in errors_and_expected {
            let produced = serde_json::to_string(&error).unwrap();
            println!("{}", produced);
            assert_eq!(expected, produced);
        }
    }
}
