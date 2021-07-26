use crate::api::signaling::mcu::{MediaSessionKey, MediaSessionType};
use crate::api::signaling::ParticipantId;
use janus_client::TrickleCandidate;
use serde::Serialize;

#[derive(Debug, Serialize)]
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

    /// Contains human readable error message about what request failed
    #[serde(rename = "error")]
    Error { text: &'static str },
}

#[derive(Debug, Serialize)]
pub struct Sdp {
    /// The payload of the sdp message
    pub sdp: String,

    #[serde(flatten)]
    pub source: Source,
}

#[derive(Debug, Serialize)]
pub struct SdpCandidate {
    /// The payload of the sdp message
    pub candidate: TrickleCandidate,

    #[serde(flatten)]
    pub source: Source,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkDirection {
    Upstream,
    Downstream,
}

#[derive(Debug, Serialize)]
pub struct Link {
    pub direction: LinkDirection,
    #[serde(flatten)]
    pub source: Source,
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
        let expected = r#"{"message":"sdp_candidate","candidate":{"sdpMid":"1","sdpMLineIndex":1,"candidate":"candidate:4 1 UDP 123456 192.168.178.1 123456 typ host"},"source":"00000000-0000-0000-0000-000000000000","media_session_type":"video"}"#;

        let produced = serde_json::to_string(&Message::SdpCandidate(SdpCandidate {
            candidate: TrickleCandidate {
                sdp_m_id: "1".into(),
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
    fn error() {
        let expected = r#"{"message":"error","text":"Error!"}"#;

        let produced = serde_json::to_string(&Message::Error { text: "Error!" }).unwrap();

        assert_eq!(expected, produced);
    }
}
