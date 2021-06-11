use crate::api::signaling::local::Participant;
use crate::api::signaling::mcu::MediaSessionType;
use crate::api::signaling::ParticipantId;
use janus_client::TrickleCandidate;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(tag = "message")]
pub enum Message {
    #[serde(rename = "join_success")]
    JoinSuccess(JoinSuccess),

    /// State change of this participant
    #[serde(rename = "update")]
    Update(Participant),
    /// A participant that joined the room
    #[serde(rename = "joined")]
    Joined(Participant),
    /// This participant left the room
    #[serde(rename = "left")]
    Left(AssociatedParticipant),

    /// SDP Offer, renegotiate publish
    #[serde(rename = "sdp_offer")]
    SdpOffer(Sdp),
    /// SDP Answer, start the publish/subscription
    #[serde(rename = "sdp_answer")]
    SdpAnswer(Sdp),
    /// SDP Candidate, used for ICE negotiation
    #[serde(rename = "sdp_candidate")]
    SdpCandidate(SdpCandidate),

    #[serde(rename = "error")]
    Error { text: &'static str },
}

#[derive(Debug, Serialize)]
pub struct JoinSuccess {
    pub id: ParticipantId,
    pub participants: Vec<Participant>,
}

#[derive(Debug, Serialize)]
pub struct AssociatedParticipant {
    pub id: ParticipantId,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_success() {
        let expected = r#"{"message":"join_success","id":"00000000-0000-0000-0000-000000000000","participants":[]}"#;

        let produced = serde_json::to_string(&Message::JoinSuccess(JoinSuccess {
            id: ParticipantId::nil(),
            participants: vec![],
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn update() {
        let expected = r#"{"message":"update","id":"00000000-0000-0000-0000-000000000000","display_name":"Hans","publishing":{}}"#;

        let produced = serde_json::to_string(&Message::Update(Participant::new(
            ParticipantId::nil(),
            "Hans".into(),
        )))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn joined() {
        let expected = r#"{"message":"joined","id":"00000000-0000-0000-0000-000000000000","display_name":"Hans","publishing":{}}"#;

        let produced = serde_json::to_string(&Message::Joined(Participant::new(
            ParticipantId::nil(),
            "Hans".into(),
        )))
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn left() {
        let expected = r#"{"message":"left","id":"00000000-0000-0000-0000-000000000000"}"#;

        let produced = serde_json::to_string(&Message::Left(AssociatedParticipant {
            id: ParticipantId::nil(),
        }))
        .unwrap();

        assert_eq!(expected, produced);
    }

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
    fn error() {
        let expected = r#"{"message":"error","text":"Error!"}"#;

        let produced = serde_json::to_string(&Message::Error { text: "Error!" }).unwrap();

        assert_eq!(expected, produced);
    }
}
