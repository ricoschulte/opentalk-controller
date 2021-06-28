use super::MediaSessionState;
use crate::api::signaling::mcu::MediaSessionType;
use crate::api::signaling::ParticipantId;
use janus_client::TrickleCandidate;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "action")]
pub enum Message {
    /// The participant successfully established a stream
    #[serde(rename = "publish_complete")]
    PublishComplete(MediaSessionInfo),

    /// The participants publish stream has stopped (for whatever reason)
    #[serde(rename = "unpublish")]
    Unpublish(AssociatedMediaSession),

    /// The participant updates its stream-state
    ///
    /// This can be mute/unmute of video or audio
    #[serde(rename = "update_media_session")]
    UpdateMediaSession(MediaSessionInfo),

    /// SDP offer
    #[serde(rename = "publish")]
    Publish(TargetedSdp),

    /// SDP Answer
    #[serde(rename = "sdp_answer")]
    SdpAnswer(TargetedSdp),

    /// SDP Candidate
    #[serde(rename = "sdp_candidate")]
    SdpCandidate(TargetedCandidate),

    /// SDP EndOfCandidate
    #[serde(rename = "sdp_end_of_candidates")]
    SdpEndOfCandidates(Target),

    /// SDP request offer
    #[serde(rename = "subscribe")]
    Subscribe(Target),
}

#[derive(Debug, Deserialize)]
pub struct AssociatedMediaSession {
    /// The stream type that has been published
    pub media_session_type: MediaSessionType,
}

#[derive(Debug, Deserialize)]
pub struct MediaSessionInfo {
    /// The stream type that has been published
    pub media_session_type: MediaSessionType,

    /// The current state of the session
    pub media_session_state: MediaSessionState,
}

#[derive(Debug, Deserialize)]
pub struct TargetedSdp {
    /// The payload of the sdp message
    pub sdp: String,

    /// The target of this SDP message.
    #[serde(flatten)]
    pub target: Target,
}

#[derive(Debug, Deserialize)]
pub struct TargetedCandidate {
    /// The payload of the sdp message
    pub candidate: TrickleCandidate,

    /// The target of this Candidate
    #[serde(flatten)]
    pub target: Target,
}

#[derive(Debug, Deserialize)]
pub struct Target {
    /// The target of this SDP message.
    ///
    /// If the own ID is specified it is used to negotiate the publish stream.
    pub target: ParticipantId,

    /// The type of stream
    pub media_session_type: MediaSessionType,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn publish() {
        let json = r#"
        {
            "action": "publish_complete",
            "media_session_type": "video",
            "media_session_state": {
                "audio": false,
                "video": false
            }
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::PublishComplete(MediaSessionInfo {
            media_session_type,
            media_session_state,
        }) = msg
        {
            assert_eq!(media_session_type, MediaSessionType::Video);
            assert_eq!(media_session_state.audio, false);
            assert_eq!(media_session_state.video, false);
        } else {
            panic!()
        }
    }

    #[test]
    fn unpublish() {
        let json = r#"
        {
            "action": "unpublish",
            "media_session_type": "video"
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::Unpublish(AssociatedMediaSession { media_session_type }) = msg {
            assert_eq!(media_session_type, MediaSessionType::Video);
        } else {
            panic!()
        }
    }

    #[test]
    fn update_media_session() {
        let json = r#"
        {
            "action": "update_media_session",
            "media_session_type": "video",
            "media_session_state": {
                "audio": true,
                "video": false
            }
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::UpdateMediaSession(MediaSessionInfo {
            media_session_type,
            media_session_state,
        }) = msg
        {
            assert_eq!(media_session_type, MediaSessionType::Video);
            assert_eq!(media_session_state.audio, true);
            assert_eq!(media_session_state.video, false);
        } else {
            panic!()
        }
    }

    #[test]
    fn offer() {
        let json = r#"
        {
            "action": "publish",
            "sdp": "v=0\r\n...",
            "target": "00000000-0000-0000-0000-000000000000",
            "media_session_type": "video"
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::Publish(TargetedSdp {
            sdp,
            target:
                Target {
                    target,
                    media_session_type,
                },
        }) = msg
        {
            assert_eq!(sdp, "v=0\r\n...");
            assert_eq!(target, ParticipantId::nil());
            assert_eq!(media_session_type, MediaSessionType::Video);
        } else {
            panic!()
        }
    }

    #[test]
    fn answer() {
        let json = r#"
        {
            "action": "sdp_answer",
            "sdp": "v=0\r\n...",
            "target": "00000000-0000-0000-0000-000000000000",
            "media_session_type": "video"
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::SdpAnswer(TargetedSdp {
            sdp,
            target:
                Target {
                    target,
                    media_session_type,
                },
        }) = msg
        {
            assert_eq!(sdp, "v=0\r\n...");
            assert_eq!(target, ParticipantId::nil());
            assert_eq!(media_session_type, MediaSessionType::Video);
        } else {
            panic!()
        }
    }

    #[test]
    fn candidate() {
        let json = r#"
        {
            "action": "sdp_candidate",
            "candidate": {
                "candidate": "candidate:4 1 UDP 123456 192.168.178.1 123456 typ host",
                "sdpMid": "1",
                "sdpMLineIndex": 1
            },
            "target": "00000000-0000-0000-0000-000000000000",
            "media_session_type": "video"
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::SdpCandidate(TargetedCandidate {
            candidate:
                TrickleCandidate {
                    sdp_m_id,
                    sdp_m_line_index,
                    candidate,
                },
            target:
                Target {
                    target,
                    media_session_type,
                },
        }) = msg
        {
            assert_eq!(sdp_m_id, "1");
            assert_eq!(sdp_m_line_index, 1);
            assert_eq!(
                candidate,
                "candidate:4 1 UDP 123456 192.168.178.1 123456 typ host"
            );
            assert_eq!(target, ParticipantId::nil());
            assert_eq!(media_session_type, MediaSessionType::Video);
        } else {
            panic!()
        }
    }

    #[test]
    fn end_of_candidates() {
        let json = r#"
        {
            "action": "sdp_end_of_candidates",
            "target": "00000000-0000-0000-0000-000000000000",
            "media_session_type": "video"
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::SdpEndOfCandidates(Target {
            target,
            media_session_type,
        }) = msg
        {
            assert_eq!(target, ParticipantId::nil());
            assert_eq!(media_session_type, MediaSessionType::Video);
        } else {
            panic!()
        }
    }

    #[test]
    fn request_offer() {
        let json = r#"
        {
            "action": "subscribe",
            "target": "00000000-0000-0000-0000-000000000000",
            "media_session_type": "video"
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::Subscribe(Target {
            target,
            media_session_type,
        }) = msg
        {
            assert_eq!(target, ParticipantId::nil());
            assert_eq!(media_session_type, MediaSessionType::Video);
        } else {
            panic!()
        }
    }
}
