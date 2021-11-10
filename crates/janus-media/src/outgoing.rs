use crate::mcu::{MediaSessionKey, MediaSessionType};
use controller::prelude::*;
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

    /// Signals that a webrtc connection has been established
    #[serde(rename = "webrtc_up")]
    WebRtcUp(Source),

    /// Signals that a webrtc connection has been disconnected/destryoed by janus
    ///
    /// This message can, but wont always be received when a participant disconnects
    #[serde(rename = "webrtc_down")]
    WebRtcDown(Source),

    /// A webrtc connection experienced package loss
    #[serde(rename = "webrtc_slow")]
    WebRtcSlow(Link),

    #[serde(rename = "focus_update")]
    FocusUpdate(FocusUpdate),

    /// Contains a error about what request failed. See [`Error`]
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
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn sdp_offer() {
        let expected = r#"{"message":"sdp_offer","sdp":"v=0...","source":"00000000-0000-0000-0000-000000000000","media_session_type":"video"}"#;

        let produced = serde_json::to_string(&Message::SdpOffer(Sdp {
            sdp: "v=0...".into(),
            source: Source {
                source: ParticipantId::nil(),
                media_session_type: MediaSessionType::Video,
            },
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn sdp_answer() {
        let expected = r#"{"message":"sdp_answer","sdp":"v=0...","source":"00000000-0000-0000-0000-000000000000","media_session_type":"video"}"#;

        let produced = serde_json::to_string(&Message::SdpAnswer(Sdp {
            sdp: "v=0...".into(),
            source: Source {
                source: ParticipantId::nil(),
                media_session_type: MediaSessionType::Video,
            },
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn sdp_candidate() {
        let expected = r#"{"message":"sdp_candidate","candidate":{"sdpMLineIndex":1,"candidate":"candidate:4 1 UDP 123456 192.168.178.1 123456 typ host"},"source":"00000000-0000-0000-0000-000000000000","media_session_type":"video"}"#;

        let produced = serde_json::to_string(&Message::SdpCandidate(SdpCandidate {
            candidate: TrickleCandidate {
                sdp_m_line_index: 1,
                candidate: "candidate:4 1 UDP 123456 192.168.178.1 123456 typ host".into(),
            },
            source: Source {
                source: ParticipantId::nil(),
                media_session_type: MediaSessionType::Video,
            },
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn test_webrtc_up() {
        let expected = r#"{"message":"webrtc_up","source":"00000000-0000-0000-0000-000000000000","media_session_type":"video"}"#;

        let produced = serde_json::to_string(&Message::WebRtcUp(Source {
            source: ParticipantId::nil(),
            media_session_type: MediaSessionType::Video,
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn test_webrtc_down() {
        let expected = r#"{"message":"webrtc_down","source":"00000000-0000-0000-0000-000000000000","media_session_type":"video"}"#;

        let produced = serde_json::to_string(&Message::WebRtcDown(Source {
            source: ParticipantId::nil(),
            media_session_type: MediaSessionType::Video,
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn test_webrtc_slow() {
        let expected = r#"{"message":"webrtc_slow","direction":"upstream","source":"00000000-0000-0000-0000-000000000000","media_session_type":"video"}"#;

        let produced = serde_json::to_string(&Message::WebRtcSlow(Link {
            direction: LinkDirection::Upstream,
            source: Source {
                source: ParticipantId::nil(),
                media_session_type: MediaSessionType::Video,
            },
        }))
        .unwrap();

        assert_eq!(expected, produced);
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
