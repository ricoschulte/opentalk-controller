//! Outgoing VideoRoom plugin datatypes
//!
use std::path::PathBuf;
use crate::{
    incoming,
    outgoing::PluginBody,
    types::{is_default, AudioCodec, RoomId, VideoCodec},
    FeedId, PluginRequest,
};
use serde::{self, Serialize};

/// Plugin request body for the videoroom plugin
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "request")]
pub enum VideoRoomPluginBody {
    /// Create request
    #[serde(rename = "create")]
    Create(VideoRoomPluginCreate),
    /// Configure request
    #[serde(rename = "configure")]
    Configure(VideoRoomPluginConfigure),
    /// Join request
    #[serde(rename = "join")]
    Join(VideoRoomPluginJoin),
    /// Start request
    #[serde(rename = "start")]
    Start(VideoRoomPluginStart),
    /// List request
    #[serde(rename = "list")]
    ListRooms(VideoRoomPluginListRooms),
    /// Destroy request
    #[serde(rename = "destroy")]
    Destroy(VideoRoomPluginDestroy),
    /// Publish request
    #[serde(rename = "publish")]
    Publish(VideoRoomPluginPublish),
}

#[derive(Debug, Clone, Serialize)]
pub struct VideoRoomPluginListRooms;

impl PluginRequest for VideoRoomPluginListRooms {
    type PluginResponse = incoming::VideoRoomPluginDataSuccess;
}

impl From<VideoRoomPluginListRooms> for PluginBody {
    fn from(value: VideoRoomPluginListRooms) -> Self {
        PluginBody::VideoRoom(VideoRoomPluginBody::ListRooms(value))
    }
}

/// Publish call
///
/// See [Janus Videoroom Plugin Docs for publishing](https://janus.conf.meetecho.com/docs/videoroom.html#vroompub) for more information
#[derive(Debug, Clone, Serialize)]
pub struct VideoRoomPluginPublish {
    /// true|false, depending on whether or not audio should be relayed; true by default
    #[serde(skip_serializing_if = "is_default")]
    pub audio: bool,
    /// true|false, depending on whether or not video should be relayed; true by default
    #[serde(skip_serializing_if = "is_default")]
    pub video: bool,
    /// audio codec to prefer among the negotiated ones; optional
    #[serde(skip_serializing_if = "is_default")]
    pub audiocodec: Option<AudioCodec>,
    /// video codec to prefer among the negotiated ones; optional
    #[serde(skip_serializing_if = "is_default")]
    pub videocodec: Option<VideoCodec>,
    /// bitrate cap to return via REMB; 0 by default, means no limit, in bit/s
    #[serde(skip_serializing_if = "is_default")]
    pub bitrate: u64,
    /// true|false, whether this publisher should be recorded or not; false by default
    #[serde(skip_serializing_if = "is_default")]
    pub record: bool,
    /// if recording, the base path/file to use for the recording files; optional
    #[serde(skip_serializing_if = "is_default")]
    pub filename: Option<PathBuf>,
    /// new display name to use in the room; optional
    #[serde(skip_serializing_if = "is_default")]
    pub display: Option<String>,
    /// new audio_active_packets to overwrite in the room one; 100 by default
    #[serde(skip_serializing_if = "is_default")]
    pub audio_active_packets: u64,
    /// new audio_level_average to overwrite the room one; 25 by default
    #[serde(skip_serializing_if = "is_default")]
    pub audio_level_average: u64,
}

impl Default for VideoRoomPluginPublish {
    fn default() -> Self {
        Self {
            audio: true,
            video: true,
            audiocodec: None,
            videocodec: None,
            bitrate: 0,
            record: false,
            filename: None,
            display: None,
            audio_active_packets: 100,
            audio_level_average: 25,
        }
    }
}

impl PluginRequest for VideoRoomPluginPublish {
    type PluginResponse = incoming::VideoRoomPluginEventConfigured;
}

impl From<VideoRoomPluginPublish> for PluginBody {
    fn from(value: VideoRoomPluginPublish) -> Self {
        PluginBody::VideoRoom(VideoRoomPluginBody::Publish(value))
    }
}

/// Configure call for publishing
///
/// See [Janus Videoroom Plugin Docs for publishing](https://janus.conf.meetecho.com/docs/videoroom.html#vroompub) for more information
#[derive(Debug, Default, Clone, Serialize)]
pub struct VideoRoomPluginConfigurePublisher {
    /// true|false, depending on whether or not audio should be relayed; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<bool>,
    /// true|false, depending on whether or not video should be relayed; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<bool>,
    /// true|false, depending on whether or not data should be relayed; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<bool>,
    /// bitrate cap to return via REMB; optional, overrides the global room value if present (unless bitrate_cap is set), in bit/s
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<u64>,
    /// true|false, whether we should send this publisher a keyframe request, optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyframe: Option<bool>,
    /// true|false, whether this publisher should be recorded or not; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<bool>,
    /// if recording, the base path/file to use for the recording files; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<PathBuf>,
    /// new display name to use in the room; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    /// overrides the room audio_active_packets for this user; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_active_packets: Option<u64>,
    /// Overrides the room audio_level_average for this user; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_level_average: Option<u64>,
}

impl PluginRequest for VideoRoomPluginConfigurePublisher {
    type PluginResponse = incoming::VideoRoomPluginEventConfigured;
}

impl From<VideoRoomPluginConfigurePublisher> for PluginBody {
    fn from(value: VideoRoomPluginConfigurePublisher) -> Self {
        PluginBody::VideoRoom(VideoRoomPluginBody::Configure(
            VideoRoomPluginConfigure::Publisher(value),
        ))
    }
}

/// Configure call for subscribing
///
/// See [Janus Videoroom Plugin Docs for subscribing](https://janus.conf.meetecho.com/docs/videoroom.html#vroomsub) for more information
#[derive(Debug, Clone, Default, Serialize)]
pub struct VideoRoomPluginConfigureSubscriber {
    /// true|false, depending on whether audio should be relayed or not; optional!!
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<bool>,
    /// true|false, depending on whether video should be relayed or not; optional!!
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<bool>,
    /// true|false, depending on whether datachannel messages should be relayed or not; optional!!
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<bool>,
    /// substream to receive (0-2), in case simulcasting is enabled; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub substream: Option<u8>,
    /// temporal layers to receive (0-2), in case simulcasting is enabled; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal: Option<u8>,
    /// How much time (in us, default 250000) without receiving packets will make us drop to the substream below
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spatial_layer: Option<bool>,
    /// spatial layer to receive (0-2), in case VP9-SVC is enabled; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal_layer: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_active_packets: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_level_average: Option<u64>,
}

impl PluginRequest for VideoRoomPluginConfigureSubscriber {
    type PluginResponse = incoming::VideoRoomPluginEventConfigured;
}

impl From<VideoRoomPluginConfigureSubscriber> for PluginBody {
    fn from(value: VideoRoomPluginConfigureSubscriber) -> Self {
        PluginBody::VideoRoom(VideoRoomPluginBody::Configure(
            VideoRoomPluginConfigure::Subscriber(value),
        ))
    }
}

/// Configure
///
/// According to Janus the publish request can also be use to start publishing, instead of this variant
/// The difference is using this you can request keyframes and you are not able to define the audio/video codec
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum VideoRoomPluginConfigure {
    // videoroom.htm for subscribingl#vroompub
    Publisher(VideoRoomPluginConfigurePublisher),
    // videoroom.html#vroomsub
    Subscriber(VideoRoomPluginConfigureSubscriber),
}

/// Create call
///
/// See [Janus Videoroom Plugin Docs](https://janus.conf.meetecho.com/docs/videoroom.html#sfuapi) for more information
// todo add and document remaining fields
#[derive(Debug, Default, Clone, Serialize)]
pub struct VideoRoomPluginCreate {
    /// Description of the room
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publishers: Option<u64>,
    /// Whether the video-orientation RTP extension must be negotiated/used or not
    /// for new publishers, defaults to true if not set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub videoorient_ext: Option<bool>,
    /// Private rooms don't appear when you do a 'list' request, defaults to false if not set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_private: Option<bool>,
    /// Optional password needed for manipulating (e.g. destroying) the room
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    /// Whether all participants are required to publish and subscribe
    /// using end-to-end media encryption, e.g., via Insertable Streams; defaults to false if not set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_e2ee: Option<bool>,
    /// max video bitrate for senders, in bit/s
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<u64>,
    /// whether the above cap should act as a limit to dynamic bitrate changes by publishers, default=false
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate_cap: Option<bool>,
}

impl PluginRequest for VideoRoomPluginCreate {
    type PluginResponse = incoming::VideoRoomPluginDataCreated;
}

impl From<VideoRoomPluginCreate> for PluginBody {
    fn from(value: VideoRoomPluginCreate) -> Self {
        PluginBody::VideoRoom(VideoRoomPluginBody::Create(value))
    }
}

/// Join call for subscribing
///
/// See [Janus Videoroom Plugin Docs for subscribing](https://janus.conf.meetecho.com/docs/videoroom.html#vroomsub) for more information
// todo figure out how to make this better regarding defaults.
// Add a Builder for this Requests that have some mandatory fields but a lot of optional?
#[derive(Debug, Clone, Serialize)]
pub struct VideoRoomPluginJoinSubscriber {
    /// unique ID of the room to subscribe in; mandatory
    pub room: RoomId,
    /// unique ID of the publisher to subscribe to; mandatory
    pub feed: FeedId,
    /// unique ID of the publisher that originated this request; optional, unless mandated by the room configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_id: Option<u64>,
    /// true|false, depending on whether or not the PeerConnection should be automatically closed when the publisher leaves; true by default
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_pc: Option<bool>,
    /// true|false, depending on whether or not audio should be relayed; true by default
    #[serde(skip_serializing_if = "Option::is_none")]
    /// true|false, depending on whether or not video should be relayed; true by default
    pub audio: Option<bool>,
    /// true|false, depending on whether or not data should be relayed; true by default
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<bool>,
    /// true|false; whether or not audio should be negotiated; true by default if the publisher has audio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<bool>,
    /// true|false; whether or not video should be negotiated; true by default if the publisher has video
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer_audio: Option<bool>,
    /// true|false; whether or not datachannels should be negotiated; true by default if the publisher has datachannels
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer_video: Option<bool>,
    /// true|false; whether or not datachannels should be negotiated; true by default if the publisher has datachannels
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer_data: Option<bool>,
    /// substream to receive (0-2), in case simulcasting is enabled; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub substream: Option<u8>,
    /// temporal layers to receive (0-2), in case simulcasting is enabled; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal_layer: Option<u8>,
    /// How much time (in us, default 250000) without receiving packets will make us drop to the substream below
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spatial_layer: Option<u8>,
    /// spatial layer to receive (0-2), in case VP9-SVC is enabled; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal: Option<u64>,
}

impl PluginRequest for VideoRoomPluginJoinSubscriber {
    type PluginResponse = incoming::VideoRoomPluginDataAttached;
}

impl From<VideoRoomPluginJoinSubscriber> for PluginBody {
    fn from(value: VideoRoomPluginJoinSubscriber) -> Self {
        PluginBody::VideoRoom(VideoRoomPluginBody::Join(VideoRoomPluginJoin::Subscriber(
            value,
        )))
    }
}

/// Join call for publishing
///
/// See [Janus Videoroom Plugin Docs for publishing](https://janus.conf.meetecho.com/docs/videoroom.html#vroompub) for more information
#[derive(Debug, Clone, Serialize)]
pub struct VideoRoomPluginJoinPublisher {
    /// unique ID of the room to join
    pub room: RoomId,
    /// unique ID to register for the publisher; optional, will be chosen by the plugin if missing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// display name for the publisher; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    /// invitation token, in case the room has an ACL; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl PluginRequest for VideoRoomPluginJoinPublisher {
    type PluginResponse = incoming::VideoRoomPluginDataJoined;
}

impl From<VideoRoomPluginJoinPublisher> for PluginBody {
    fn from(value: VideoRoomPluginJoinPublisher) -> Self {
        PluginBody::VideoRoom(VideoRoomPluginBody::Join(VideoRoomPluginJoin::Publisher(
            value,
        )))
    }
}

/// Join
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "ptype")]
pub enum VideoRoomPluginJoin {
    /// Join as a subscriber
    #[serde(rename = "subscriber")]
    Subscriber(VideoRoomPluginJoinSubscriber),
    // Join as a publisher
    #[serde(rename = "publisher")]
    Publisher(VideoRoomPluginJoinPublisher),
}

/// Destroy a room
#[derive(Debug, Clone, Serialize)]
pub struct VideoRoomPluginDestroy {
    /// unique numeric ID of the room to destroy
    pub room: RoomId,
    /// room secret, mandatory if configured
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<u64>,
    /// true|false, whether the room should be also removed from the config file, default=false
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permanent: Option<String>,
    /// invitation token, in case the room has an ACL; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl PluginRequest for VideoRoomPluginDestroy {
    type PluginResponse = incoming::VideoRoomPluginDataDestroyed;
}

impl From<VideoRoomPluginDestroy> for PluginBody {
    fn from(value: VideoRoomPluginDestroy) -> Self {
        PluginBody::VideoRoom(VideoRoomPluginBody::Destroy(value))
    }
}

/// Start
// todo IMPORTANT spreed uses a room field. According to janus, this is not needed for the start command.
#[derive(Debug, Default, Clone, Serialize)]
pub struct VideoRoomPluginStart {
    // room: RoomId
}

impl PluginRequest for VideoRoomPluginStart {
    type PluginResponse = incoming::VideoRoomPluginEventStarted;
}

impl From<VideoRoomPluginStart> for PluginBody {
    fn from(value: VideoRoomPluginStart) -> Self {
        PluginBody::VideoRoom(VideoRoomPluginBody::Start(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        outgoing::{JanusRequest, PluginBody, PluginMessage},
        HandleId, Jsep, JsepType, SessionId, TransactionId,
    };
    use pretty_assertions::assert_eq;

    #[test]
    #[cfg(feature = "videoroom")]
    fn video_room_create() {
        let reference = r#"{
            "janus":"message",
            "handle_id":234,
            "session_id":123,
            "transaction":"k3k-rulez",
            "body":{
                "request":"create",
                "description":"TestRoom|StreamType",
                "publishers":1,
                "videoorient_ext":false
            }
        }"#;
        let reference = reference
            .lines()
            .map(|s| s.trim_start())
            .collect::<String>();
        let our = JanusRequest::PluginMessage(PluginMessage {
            transaction: TransactionId::new("k3k-rulez".into()),
            session_id: SessionId::new(123),
            handle_id: HandleId::new(234),
            body: PluginBody::VideoRoom(VideoRoomPluginBody::Create(VideoRoomPluginCreate {
                description: "TestRoom|StreamType".into(),
                publishers: Some(1),
                videoorient_ext: Some(false),
                ..Default::default()
            })),
            jsep: None,
        });
        // print!("{}", serde_json::to_string_pretty(&our).unwrap());
        assert_eq!(reference, serde_json::to_string(&our).unwrap());
    }

    #[test]
    fn test_configure() {
        let reference = r#"{
            "janus":"message",
            "handle_id":2123,
            "session_id":234,
            "transaction":"k3k-goes-brr",
            "body":{
              "request":"configure",
              "audio":true,
              "video":true,
               "data":true
            },
            "jsep":{"type":"offer","sdp":"v=0"}
          }"#;
        let reference = reference
            .lines()
            .map(|s| s.trim_start())
            .collect::<String>();
        let our = JanusRequest::PluginMessage(PluginMessage {
            transaction: TransactionId::new("k3k-goes-brr".into()),
            session_id: SessionId::new(234),
            handle_id: HandleId::new(2123),
            body: PluginBody::VideoRoom(VideoRoomPluginBody::Configure(
                VideoRoomPluginConfigure::Subscriber(VideoRoomPluginConfigureSubscriber {
                    audio: Some(true),
                    video: Some(true),
                    data: Some(true),
                    ..Default::default()
                }),
            )),
            jsep: Some(Jsep {
                kind: JsepType::Offer,
                sdp: "v=0".to_string(),
                trickle: None,
            }),
        });
        assert_eq!(reference, serde_json::to_string(&our).unwrap());
    }

    #[test]
    fn test_start() {
        let reference = r#"{
            "janus":"message",
            "handle_id":2123,
            "session_id":234,
            "transaction":"k3k-goes-brr",
            "body":{
              "request":"start"
            },
            "jsep":{"type":"offer","sdp":"v=0"}
          }"#;
        let reference = reference
            .lines()
            .map(|s| s.trim_start())
            .collect::<String>();
        let our = JanusRequest::PluginMessage(PluginMessage {
            transaction: TransactionId::new("k3k-goes-brr".into()),
            session_id: SessionId::new(234),
            handle_id: HandleId::new(2123),
            body: PluginBody::VideoRoom(VideoRoomPluginBody::Start(VideoRoomPluginStart {})),
            jsep: Some(Jsep {
                kind: JsepType::Offer,
                sdp: "v=0".to_string(),
                trickle: None,
            }),
        });
        assert_eq!(reference, serde_json::to_string(&our).unwrap());
    }

    #[test]
    fn test_join_sub() {
        let reference = r#"{
            "janus":"message",
            "handle_id":2123,
            "session_id":234,
            "transaction":"k3k-goes-brr",
            "body":{
              "request":"join",
              "ptype":"subscriber",
              "room":5,
              "feed":1
            },
            "jsep":{"type":"offer","sdp":"v=0"}
          }"#;
        let reference = reference
            .lines()
            .map(|s| s.trim_start())
            .collect::<String>();
        let our = JanusRequest::PluginMessage(PluginMessage {
            transaction: TransactionId::new("k3k-goes-brr".into()),
            session_id: SessionId::new(234),
            handle_id: HandleId::new(2123),
            body: PluginBody::VideoRoom(VideoRoomPluginBody::Join(
                VideoRoomPluginJoin::Subscriber(VideoRoomPluginJoinSubscriber {
                    room: 5.into(),
                    feed: 1.into(),
                    private_id: None,
                    close_pc: None,
                    audio: None,
                    video: None,
                    data: None,
                    offer_audio: None,
                    offer_video: None,
                    offer_data: None,
                    substream: None,
                    temporal_layer: None,
                    spatial_layer: None,
                    temporal: None,
                }),
            )),
            jsep: Some(Jsep {
                kind: JsepType::Offer,
                sdp: "v=0".to_string(),
                trickle: None,
            }),
        });
        assert_eq!(reference, serde_json::to_string(&our).unwrap());
    }

    #[test]
    fn test_list() {
        let reference = r#"{
            "janus":"message",
            "handle_id":2123,
            "session_id":234,
            "transaction":"k3k-goes-brr",
            "body":{
              "request":"list"
            }
          }"#;
        let reference = reference
            .lines()
            .map(|s| s.trim_start())
            .collect::<String>();
        let our = JanusRequest::PluginMessage(PluginMessage {
            transaction: TransactionId::new("k3k-goes-brr".into()),
            session_id: SessionId::new(234),
            handle_id: HandleId::new(2123),
            body: PluginBody::VideoRoom(VideoRoomPluginBody::ListRooms(VideoRoomPluginListRooms)),
            jsep: None,
        });
        assert_eq!(reference, serde_json::to_string(&our).unwrap());
    }

    #[test]
    fn test_join_pub() {
        let reference = r#"{
            "janus":"message",
            "handle_id":2123,
            "session_id":234,
            "transaction":"k3k-goes-brr",
            "body":{
              "request":"join",
              "ptype":"publisher",
              "room":5,
              "id":1
            },
            "jsep":{"type":"offer","sdp":"v=0"}
          }"#;
        let reference = reference
            .lines()
            .map(|s| s.trim_start())
            .collect::<String>();
        let our = JanusRequest::PluginMessage(PluginMessage {
            transaction: TransactionId::new("k3k-goes-brr".into()),
            session_id: SessionId::new(234),
            handle_id: HandleId::new(2123),
            body: PluginBody::VideoRoom(VideoRoomPluginBody::Join(VideoRoomPluginJoin::Publisher(
                VideoRoomPluginJoinPublisher {
                    room: 5.into(),
                    id: Some(1),
                    display: None,
                    token: None,
                },
            ))),
            jsep: Some(Jsep {
                kind: JsepType::Offer,
                sdp: "v=0".to_string(),
                trickle: None,
            }),
        });
        assert_eq!(reference, serde_json::to_string(&our).unwrap());
    }
    #[test]
    fn test_destroy() {
        let reference = r#"{
            "janus":"message",
            "handle_id":2123,
            "session_id":234,
            "transaction":"k3k-goes-brr",
            "body":{
              "request":"destroy",
              "room":1
            },
            "jsep":{"type":"offer","sdp":"v=0"}
          }"#;
        let reference = reference
            .lines()
            .map(|s| s.trim_start())
            .collect::<String>();
        let our = JanusRequest::PluginMessage(PluginMessage {
            transaction: TransactionId::new("k3k-goes-brr".into()),
            session_id: SessionId::new(234),
            handle_id: HandleId::new(2123),
            body: PluginBody::VideoRoom(VideoRoomPluginBody::Destroy(VideoRoomPluginDestroy {
                room: 1.into(),
                secret: None,
                permanent: None,
                token: None,
            })),
            jsep: Some(Jsep {
                kind: JsepType::Offer,
                sdp: "v=0".to_string(),
                trickle: None,
            }),
        });
        assert_eq!(reference, serde_json::to_string(&our).unwrap());
    }
}
