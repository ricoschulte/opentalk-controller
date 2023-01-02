use crate::mcu::MediaSessionType;
use crate::MediaSessionState;
use controller_shared::ParticipantId;
use janus_client::TrickleCandidate;
use serde::{Deserialize, Serialize};

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

    /// A moderators request to mute one or more participants
    #[serde(rename = "moderator_mute")]
    ModeratorMute(RequestMute),

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
    Subscribe(TargetSubscribe),

    /// Grant the presenter role for a set of participants
    #[serde(rename = "grant_presenter_role")]
    GrantPresenterRole(ParticipantSelection),

    /// Revoke the presenter role for a set of participants
    #[serde(rename = "revoke_presenter_role")]
    RevokePresenterRole(ParticipantSelection),

    /// SDP request to configure subscription
    #[serde(rename = "configure")]
    Configure(TargetConfigure),
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

/// Request a number of participants to mute themselves
///
/// May only be processed if the issuer is a moderator
#[derive(Debug, Deserialize)]
pub struct RequestMute {
    /// Participants that shall be muted
    pub targets: Vec<ParticipantId>,
    /// Force mute the participant(s)
    pub force: bool,
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

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    /// The target of this message.
    ///
    /// If the own ID is specified it is used to negotiate the publish stream.
    pub target: ParticipantId,

    /// The type of stream
    pub media_session_type: MediaSessionType,
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub struct TargetSubscribe {
    /// The target of the subscription
    #[serde(flatten)]
    pub target: Target,

    /// Do not subscribe to the video stream.
    /// Primarily used for SIP.
    #[serde(default)]
    pub without_video: bool,
}

/// Give a list of participants write access to the protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantSelection {
    /// The targeted participants
    pub participant_ids: Vec<ParticipantId>,
}

#[derive(Debug, Deserialize)]
pub struct TargetConfigure {
    /// The target of this configure
    #[serde(flatten)]
    pub target: Target,

    /// New Configuration
    ///
    /// Contains the configuration changes/settings to be applied.
    pub configuration: SubscriberConfiguration,
}

#[derive(Debug, Deserialize)]
pub struct SubscriberConfiguration {
    /// Video Feed
    ///
    /// If true, will configure the the connection to receive the video stream.
    /// If false, will disable the video feed relaying.
    pub video: Option<bool>,
    /// Substream
    ///
    /// If enabled, the selected substream of the three (0-2) available
    pub substream: Option<u8>,
}

#[cfg(test)]
mod test {
    use super::*;
    use controller::prelude::*;
    use pretty_assertions::assert_eq;

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
            assert!(!media_session_state.audio);
            assert!(!media_session_state.video);
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
            assert!(media_session_state.audio);
            assert!(!media_session_state.video);
        } else {
            panic!()
        }
    }

    #[test]
    fn moderator_mute_single() {
        let json = r#"
        {
            "action": "moderator_mute",
            "targets": ["00000000-0000-0000-0000-000000000000"],
            "force": true
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::ModeratorMute(RequestMute { targets, force }) = msg {
            assert_eq!(targets, vec![ParticipantId::nil()]);
            assert!(force);
        } else {
            panic!()
        }
    }

    #[test]
    fn moderator_mute_many() {
        let json = r#"
        {
            "action": "moderator_mute",
            "targets": ["00000000-0000-0000-0000-000000000000", "00000000-0000-0000-0000-000000000001", "00000000-0000-0000-0000-000000000002"],
            "force": false
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::ModeratorMute(RequestMute { targets, force }) = msg {
            assert_eq!(
                targets,
                [
                    ParticipantId::new_test(0),
                    ParticipantId::new_test(1),
                    ParticipantId::new_test(2)
                ]
            );
            assert!(!force);
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

        if let Message::Subscribe(TargetSubscribe {
            target:
                Target {
                    target,
                    media_session_type,
                },
            without_video,
        }) = msg
        {
            assert_eq!(target, ParticipantId::nil());
            assert_eq!(media_session_type, MediaSessionType::Video);
            assert!(!without_video);
        } else {
            panic!()
        }
    }

    #[test]
    fn grant_presenter_role() {
        let json = r#"
        {
            "action": "grant_presenter_role",
            "participant_ids": ["00000000-0000-0000-0000-000000000000", "00000000-0000-0000-0000-000000000000"]
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::GrantPresenterRole(ParticipantSelection { participant_ids }) = msg {
            assert_eq!(
                participant_ids,
                vec![ParticipantId::nil(), ParticipantId::nil()]
            );
        } else {
            panic!()
        }
    }

    #[test]
    fn revoke_presenter_role() {
        let json = r#"
        {
            "action": "revoke_presenter_role",
            "participant_ids": ["00000000-0000-0000-0000-000000000000", "00000000-0000-0000-0000-000000000000"]
        }
        "#;

        let msg: Message = serde_json::from_str(json).unwrap();

        if let Message::RevokePresenterRole(ParticipantSelection { participant_ids }) = msg {
            assert_eq!(
                participant_ids,
                vec![ParticipantId::nil(), ParticipantId::nil()]
            );
        } else {
            panic!()
        }
    }
}
