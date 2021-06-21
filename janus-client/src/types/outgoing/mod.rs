//! Outgoing Datatypes
//!
//! This are the request types sent async via the websocket to Janus.

use crate::{
    error,
    types::{AudioCodec, Jsep, TransactionId, TrickleCandidate, VideoCodec},
    HandleId, JanusPlugin, SessionId,
};
#[cfg(feature = "echotest")]
use echotest::EchoPluginBody;
use serde::{self, Serialize};
#[cfg(feature = "videoroom")]
use videoroom::VideoRoomPluginBody;

#[cfg(feature = "echotest")]
pub use echotest::EchoPluginUnnamed;

#[cfg(feature = "videoroom")]
pub use videoroom::{
    VideoRoomPluginConfigure, VideoRoomPluginConfigurePublisher,
    VideoRoomPluginConfigureSubscriber, VideoRoomPluginCreate, VideoRoomPluginDestroy,
    VideoRoomPluginJoin, VideoRoomPluginJoinPublisher, VideoRoomPluginJoinSubscriber,
    VideoRoomPluginListRooms, VideoRoomPluginStart,
};

#[cfg(feature = "echotest")]
pub(crate) mod echotest;
#[cfg(feature = "videoroom")]
pub(crate) mod videoroom;

/// Ingoing and Outgoing JSON strictly typed API
#[derive(Debug, Serialize)]
#[serde(tag = "janus")]
pub(crate) enum JanusRequest {
    /// Keepalive
    #[serde(rename = "keepalive")]
    KeepAlive(KeepAlive),
    #[serde(rename = "create")]
    CreateSession(CreateSession),
    #[serde(rename = "attach")]
    AttachToPlugin(AttachToPlugin),
    #[serde(rename = "message")]
    PluginMessage(PluginMessage),
    /// Trickle request
    #[serde(rename = "trickle")]
    TrickleMessage {
        handle_id: HandleId,
        session_id: SessionId,
        transaction: TransactionId,
        #[serde(flatten)]
        trickle: TrickleMessage,
    },
    /// Destroys a handle
    Detach {
        session_id: SessionId,
        handle_id: HandleId,
        transaction: TransactionId,
    },
    /// Destroys a session
    Destroy { session_id: SessionId },
}

/// Keepalive message
#[derive(Debug, Serialize)]
pub struct KeepAlive {
    pub session_id: SessionId,
    pub(crate) transaction: TransactionId,
}

/// Creates a session
#[derive(Debug, Serialize)]
pub struct CreateSession {
    pub(crate) transaction: TransactionId,
}

/// Attaches the given session to a plugin
#[derive(Debug, Serialize)]
pub struct AttachToPlugin {
    pub plugin: JanusPlugin,
    pub session_id: SessionId,
    pub(crate) transaction: TransactionId,
}

/// Sends a message to a plugin
#[derive(Debug, Serialize)]
pub struct PluginMessage {
    pub handle_id: HandleId,
    pub session_id: SessionId,
    pub(crate) transaction: TransactionId,
    pub body: PluginBody,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsep: Option<Jsep>,
}

/// Inner trickle message. Either single or multiple candidates
#[derive(Debug, Serialize)]
pub enum TrickleMessage {
    #[serde(rename = "candidate")]
    Candidate(TrickleCandidate),
    #[serde(rename = "candidates")]
    MultipleCandidates(Vec<TrickleCandidate>),
    /// IS ALWAYS TRUE, DO NOT SEND COMPLETED==false!!!
    #[serde(rename = "candidate")]
    Completed { completed: bool },
}

impl TrickleMessage {
    /// Creates the respective variant based on the number of candidates
    pub fn new(candidates: &[TrickleCandidate]) -> Result<Self, error::Error> {
        match candidates.len() {
            0 => Err(error::Error::InvalidCandidates),
            1 => Ok(Self::Candidate(candidates[0].clone())),
            _ => Ok(Self::MultipleCandidates(candidates.into())),
        }
    }

    pub fn end() -> Self {
        Self::Completed { completed: true }
    }
}

/// Request body for request to plugins
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum PluginBody {
    #[cfg(feature = "videoroom")]
    #[serde(rename = "janus.plugin.videoroom")]
    VideoRoom(VideoRoomPluginBody),
    #[cfg(feature = "echotest")]
    #[serde(rename = "janus.plugin.echotest")]
    EchoTest(EchoPluginBody),
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::types::{outgoing::JanusRequest, HandleId, SessionId, TransactionId};
    use pretty_assertions::assert_eq;

    #[test]
    fn test_trickle() {
        let reference = r#"{
            "janus":"trickle",
            "handle_id":2123,
            "session_id":234,
            "transaction":"k3k-goes-brr",
            "candidates":[
                {
                    "sdpMid":"video",
                    "sdpMLineIndex":1,
                    "candidate":"1 2 UDP 2130706431 10.0.1.1 5001 typ host"
            },
            {
                    "sdpMid":"video",
                    "sdpMLineIndex":1,
                    "candidate":"2 1 UDP 1694498815 192.0.2.3 5000 typ srflx raddr 10.0.1.1 rport 8998"
            }
        ]
          }"#;
        let reference = reference
            .lines()
            .map(|s| s.trim_start())
            .collect::<String>();
        let our = JanusRequest::TrickleMessage {
            transaction: TransactionId::new("k3k-goes-brr".into()),
            session_id: SessionId::new(234),
            handle_id: HandleId::new(2123),
            trickle: TrickleMessage::new(&vec![
                TrickleCandidate {
                    sdp_m_id: "video".to_owned(),
                    sdp_m_line_index: 1,
                    candidate: "1 2 UDP 2130706431 10.0.1.1 5001 typ host".to_owned(),
                },
                TrickleCandidate {
                    sdp_m_id: "video".to_owned(),
                    sdp_m_line_index: 1,
                    candidate:
                        "2 1 UDP 1694498815 192.0.2.3 5000 typ srflx raddr 10.0.1.1 rport 8998"
                            .to_owned(),
                },
            ])
            .unwrap(),
        };
        assert_eq!(reference, serde_json::to_string(&our).unwrap());
    }
}
