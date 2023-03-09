// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! Outgoing VideoRoom plugin datatypes
//!
use crate::{
    incoming,
    outgoing::PluginBody,
    types::{is_default, AudioCodec, RoomId, VideoCodec},
    FeedId, PluginRequest,
};
use serde::{self, Serialize, Serializer};
use std::cmp;
use std::fmt::Write;
use std::path::PathBuf;

/// Plugin request body for the videoroom plugin
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "request")]
pub enum VideoRoomPluginBody {
    /// Create request, sync
    #[serde(rename = "create")]
    Create(VideoRoomPluginCreate),
    /// Configure request, async
    #[serde(rename = "configure")]
    Configure(VideoRoomPluginConfigure),
    /// Join request, async
    #[serde(rename = "join")]
    Join(VideoRoomPluginJoin),
    /// Start request, async
    #[serde(rename = "start")]
    Start(VideoRoomPluginStart),
    /// List request, sync
    #[serde(rename = "list")]
    ListRooms(VideoRoomPluginListRooms),
    /// Destroy request, sync
    #[serde(rename = "destroy")]
    Destroy(VideoRoomPluginDestroy),
    /// Publish request, async
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
    const IS_ASYNC: bool = true;
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
    const IS_ASYNC: bool = true;
}

impl From<VideoRoomPluginConfigurePublisher> for PluginBody {
    fn from(value: VideoRoomPluginConfigurePublisher) -> Self {
        PluginBody::VideoRoom(VideoRoomPluginBody::Configure(
            VideoRoomPluginConfigure::Publisher(value),
        ))
    }
}

impl VideoRoomPluginConfigurePublisher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn audio(self, audio: Option<bool>) -> Self {
        Self { audio, ..self }
    }

    pub fn video(self, video: Option<bool>) -> Self {
        Self { video, ..self }
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
    pub spatial_layer: Option<u8>,
    /// spatial layer to receive (0-2), in case VP9-SVC is enabled; optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal_layer: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_active_packets: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_level_average: Option<u64>,
    /// Force ICE restart
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart: Option<bool>,
}

impl PluginRequest for VideoRoomPluginConfigureSubscriber {
    type PluginResponse = incoming::VideoRoomPluginEventConfigured;
    const IS_ASYNC: bool = true;
}

impl From<VideoRoomPluginConfigureSubscriber> for PluginBody {
    fn from(value: VideoRoomPluginConfigureSubscriber) -> Self {
        PluginBody::VideoRoom(VideoRoomPluginBody::Configure(
            VideoRoomPluginConfigure::Subscriber(value),
        ))
    }
}

impl VideoRoomPluginConfigureSubscriber {
    /// Returns a new VideoRoomPluginConfigureSubscriber
    ///
    /// Every optional value is initially set to None.
    pub fn new() -> VideoRoomPluginConfigureSubscriber {
        Self {
            audio: None,
            video: None,
            data: None,
            substream: None,
            temporal: None,
            fallback: None,
            temporal_layer: None,
            spatial_layer: None,
            audio_active_packets: None,
            audio_level_average: None,
            restart: None,
        }
    }

    /// Returns a new Builder for VideoRoomPluginJoinSubscriber
    ///
    /// Every optional value is initially set to None.
    pub fn builder() -> VideoRoomPluginConfigureSubscriberBuilder {
        VideoRoomPluginConfigureSubscriberBuilder(Self::new())
    }
}

/// Builder for VideoRoomPluginConfigureSubscriber
pub struct VideoRoomPluginConfigureSubscriberBuilder(VideoRoomPluginConfigureSubscriber);

impl VideoRoomPluginConfigureSubscriberBuilder {
    pub fn audio(self, enabled: Option<bool>) -> Self {
        Self(VideoRoomPluginConfigureSubscriber {
            audio: enabled,
            ..self.0
        })
    }

    pub fn video(self, enabled: Option<bool>) -> Self {
        Self(VideoRoomPluginConfigureSubscriber {
            video: enabled,
            ..self.0
        })
    }

    pub fn data(self, enabled: Option<bool>) -> Self {
        Self(VideoRoomPluginConfigureSubscriber {
            data: enabled,
            ..self.0
        })
    }

    pub fn substream(self, substream_index: Option<u8>) -> Self {
        Self(VideoRoomPluginConfigureSubscriber {
            substream: substream_index.map(|index| cmp::min(index, 2)),
            ..self.0
        })
    }

    pub fn temporal(self, temporal_layer_index: Option<u8>) -> Self {
        Self(VideoRoomPluginConfigureSubscriber {
            temporal: temporal_layer_index.map(|index| cmp::min(index, 2)),
            ..self.0
        })
    }

    pub fn fallback(self, fallback_in_microseconds: Option<u64>) -> Self {
        Self(VideoRoomPluginConfigureSubscriber {
            fallback: fallback_in_microseconds,
            ..self.0
        })
    }

    pub fn spatial_layer(self, spatial_layer_index: Option<u8>) -> Self {
        Self(VideoRoomPluginConfigureSubscriber {
            spatial_layer: spatial_layer_index.map(|index| cmp::min(index, 2)),
            ..self.0
        })
    }

    pub fn temporal_layer(self, temporal_layer_index: Option<u8>) -> Self {
        Self(VideoRoomPluginConfigureSubscriber {
            temporal_layer: temporal_layer_index.map(|index| cmp::min(index, 2)),
            ..self.0
        })
    }

    pub fn audio_active_packets(self, number_of_packets: Option<u64>) -> Self {
        Self(VideoRoomPluginConfigureSubscriber {
            audio_active_packets: number_of_packets,
            ..self.0
        })
    }

    pub fn audio_level_average(self, audio_level: Option<u64>) -> Self {
        Self(VideoRoomPluginConfigureSubscriber {
            audio_level_average: audio_level,
            ..self.0
        })
    }

    pub fn restart(self, restart: Option<bool>) -> Self {
        Self(VideoRoomPluginConfigureSubscriber { restart, ..self.0 })
    }

    pub fn build(self) -> VideoRoomPluginConfigureSubscriber {
        self.0
    }
}

/// Configure
///
/// According to Janus the publish request can also be use to start publishing, instead of this variant
/// The difference is using this you can request keyframes and you are not able to define the audio/video codec
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum VideoRoomPluginConfigure {
    // videoroom.htm for subscribing#vroompub
    Publisher(VideoRoomPluginConfigurePublisher),
    // videoroom.html#vroomsub
    Subscriber(VideoRoomPluginConfigureSubscriber),
}

/// Create a new room witht he given room settings.
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
    /// send a FIR to publishers every fir_freq seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fir_req: Option<u64>,
    /// Enable the transport wide CC RTP extension must be used for this room
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport_wide_cc_ext: Option<bool>,
    /// audio codec(s) to force on publishers, default to opus if not set
    /// can be a comma separated list in order of preference, e.g., opus,pcmu
    #[serde(serialize_with = "comma_seperated")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub audiocodec: Vec<AudioCodec>,
    /// video codec(s) to force on publishers, default=vp8
    /// can be a comma separated list in order of preference, e.g., vp9,vp8,h264
    #[serde(serialize_with = "comma_seperated")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub videocodec: Vec<VideoCodec>,
    /// VP9-specific profile to prefer (e.g., "2" for "profile-id=2")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vp9_profile: Option<String>,
    /// H.264-specific profile to prefer (e.g., "42e01f" for "profile-level-id=42e01f")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h264_profile: Option<String>,
    /// whether inband FEC must be negotiated; only works for Opus, default to false if not set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opus_fec: Option<bool>,
    /// whether SVC support must be enabled; only works for VP9, default to false if not set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_svc: Option<bool>,
    /// whether the ssrc-audio-level RTP extension must
    /// be negotiated/used or not for new publishers, default is true if not set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audiolevel_ext: Option<bool>,
    /// whether to emit event to other users or not, default is false if not set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audiolevel_event: Option<bool>,
    /// number of packets with audio level, default is 100, 2 seconds if not set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_active_packets: Option<i64>,
    /// average value of audio level, 127=muted, 0='too loud', default=25 if not set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_level_average: Option<i64>,
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
    pub audio: Option<bool>,
    /// true|false, depending on whether or not video should be relayed; true by default
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<bool>,
    /// true|false, depending on whether or not data should be relayed; true by default
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<bool>,
    /// true|false; whether or not audio should be negotiated; true by default if the publisher has audio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer_audio: Option<bool>,
    /// true|false; whether or not video should be negotiated; true by default if the publisher has video
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
    pub temporal: Option<u8>,
}

impl PluginRequest for VideoRoomPluginJoinSubscriber {
    type PluginResponse = incoming::VideoRoomPluginDataAttached;
    const IS_ASYNC: bool = true;
}

impl From<VideoRoomPluginJoinSubscriber> for PluginBody {
    fn from(value: VideoRoomPluginJoinSubscriber) -> Self {
        PluginBody::VideoRoom(VideoRoomPluginBody::Join(VideoRoomPluginJoin::Subscriber(
            value,
        )))
    }
}

impl VideoRoomPluginJoinSubscriber {
    /// Returns a new VideoRoomPluginJoinSubscriber
    ///
    /// Every optional value is initially set to None.
    pub fn new(room: RoomId, feed: FeedId) -> VideoRoomPluginJoinSubscriber {
        Self {
            room,
            feed,
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
        }
    }

    /// Returns a new Builder for VideoRoomPluginJoinSubscriber
    ///
    /// Every optional value is initially set to None.
    pub fn builder(room: RoomId, feed: FeedId) -> VideoRoomPluginJoinSubscriberBuilder {
        VideoRoomPluginJoinSubscriberBuilder(Self::new(room, feed))
    }
}

/// Builder for VideoRoomPluginJoinSubscriber
pub struct VideoRoomPluginJoinSubscriberBuilder(VideoRoomPluginJoinSubscriber);

impl VideoRoomPluginJoinSubscriberBuilder {
    pub fn private_id(self, id: Option<u64>) -> Self {
        Self(VideoRoomPluginJoinSubscriber {
            private_id: id,
            ..self.0
        })
    }

    pub fn close_pc(self, close: Option<bool>) -> Self {
        Self(VideoRoomPluginJoinSubscriber {
            close_pc: close,
            ..self.0
        })
    }

    pub fn audio(self, enabled: Option<bool>) -> Self {
        Self(VideoRoomPluginJoinSubscriber {
            audio: enabled,
            ..self.0
        })
    }

    pub fn video(self, enabled: Option<bool>) -> Self {
        Self(VideoRoomPluginJoinSubscriber {
            video: enabled,
            ..self.0
        })
    }

    pub fn data(self, enabled: Option<bool>) -> Self {
        Self(VideoRoomPluginJoinSubscriber {
            data: enabled,
            ..self.0
        })
    }

    pub fn offer_audio(self, offer: Option<bool>) -> Self {
        Self(VideoRoomPluginJoinSubscriber {
            offer_audio: offer,
            ..self.0
        })
    }

    pub fn offer_video(self, offer: Option<bool>) -> Self {
        Self(VideoRoomPluginJoinSubscriber {
            offer_video: offer,
            ..self.0
        })
    }

    pub fn offer_data(self, offer: Option<bool>) -> Self {
        Self(VideoRoomPluginJoinSubscriber {
            offer_data: offer,
            ..self.0
        })
    }

    pub fn substream(self, substream_index: Option<u8>) -> Self {
        Self(VideoRoomPluginJoinSubscriber {
            substream: substream_index.map(|index| cmp::min(index, 2)),
            ..self.0
        })
    }

    pub fn temporal_layer(self, temporal_layer_index: Option<u8>) -> Self {
        Self(VideoRoomPluginJoinSubscriber {
            temporal_layer: temporal_layer_index.map(|index| cmp::min(index, 2)),
            ..self.0
        })
    }

    pub fn spatial_layer(self, spatial_layer_index: Option<u8>) -> Self {
        Self(VideoRoomPluginJoinSubscriber {
            spatial_layer: spatial_layer_index.map(|index| cmp::min(index, 2)),
            ..self.0
        })
    }

    pub fn temporal(self, temporal_layer_index: Option<u8>) -> Self {
        Self(VideoRoomPluginJoinSubscriber {
            temporal: temporal_layer_index.map(|index| cmp::min(index, 2)),
            ..self.0
        })
    }

    pub fn build(self) -> VideoRoomPluginJoinSubscriber {
        self.0
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
    const IS_ASYNC: bool = true;
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
    const IS_ASYNC: bool = true;
}

impl From<VideoRoomPluginStart> for PluginBody {
    fn from(value: VideoRoomPluginStart) -> Self {
        PluginBody::VideoRoom(VideoRoomPluginBody::Start(value))
    }
}

fn comma_seperated<S, T>(items: &[T], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: std::fmt::Display,
{
    let mut iter = items.iter();

    if let Some(first) = iter.next() {
        let mut string = first.to_string();

        for el in iter {
            let _ = write!(string, ",{el}");
        }

        serializer.serialize_str(&string)
    } else {
        serializer.serialize_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_eq_json;
    use crate::types::{
        outgoing::{JanusRequest, PluginBody, PluginMessage},
        HandleId, Jsep, JsepType, SessionId, VideoCodec,
    };
    use pretty_assertions::assert_eq;

    #[test]
    #[cfg(feature = "videoroom")]
    fn video_room_create() {
        let plugin_message = JanusRequest::PluginMessage(PluginMessage {
            session_id: SessionId::new(123),
            handle_id: HandleId::new(234),
            body: PluginBody::VideoRoom(VideoRoomPluginBody::Create(VideoRoomPluginCreate {
                description: "TestRoom|StreamType".into(),
                publishers: Some(1),
                videoorient_ext: Some(false),
                videocodec: vec![],
                ..Default::default()
            })),
            jsep: None,
        });

        assert_eq_json!(
            plugin_message,
            {
                "janus": "message",
                "handle_id": 234,
                "session_id": 123,
                "body": {
                    "request": "create",
                    "description": "TestRoom|StreamType",
                    "publishers": 1,
                    "videoorient_ext": false
                }
            }
        );
    }

    #[test]
    fn test_configure() {
        let plugin_message = JanusRequest::PluginMessage(PluginMessage {
            session_id: SessionId::new(234),
            handle_id: HandleId::new(2123),
            body: PluginBody::VideoRoom(VideoRoomPluginBody::Configure(
                VideoRoomPluginConfigure::Subscriber(
                    VideoRoomPluginConfigureSubscriber::builder()
                        .audio(Some(true))
                        .video(Some(true))
                        .data(Some(true))
                        .build(),
                ),
            )),
            jsep: Some(Jsep {
                kind: JsepType::Offer,
                sdp: "v=0".to_string(),
                trickle: None,
            }),
        });

        assert_eq_json!(
            plugin_message,
            {
                "janus": "message",
                "handle_id": 2123,
                "session_id": 234,
                "body": {
                  "request": "configure",
                  "audio": true,
                  "video": true,
                  "data": true
                },
                "jsep": {
                    "type": "offer",
                    "sdp": "v=0"
                }
            }
        );
    }

    #[test]
    fn test_video_room_plugin_configure_subscriber_builder() {
        let request = VideoRoomPluginConfigureSubscriber::builder()
            .audio(Some(true))
            .video(Some(true))
            .data(Some(false))
            .substream(Some(4))
            .temporal(Some(4))
            .fallback(Some(42))
            .spatial_layer(Some(4))
            .temporal_layer(Some(4))
            .audio_active_packets(Some(42))
            .audio_level_average(Some(42))
            .build();

        assert_eq!(request.audio, Some(true));
        assert_eq!(request.video, Some(true));
        assert_eq!(request.data, Some(false));
        assert_eq!(request.substream, Some(2));
        assert_eq!(request.temporal, Some(2));
        assert_eq!(request.fallback, Some(42));
        assert_eq!(request.spatial_layer, Some(2));
        assert_eq!(request.temporal_layer, Some(2));
        assert_eq!(request.audio_active_packets, Some(42));
        assert_eq!(request.audio_level_average, Some(42));
    }

    #[test]
    fn test_start() {
        let plugin_message = JanusRequest::PluginMessage(PluginMessage {
            session_id: SessionId::new(234),
            handle_id: HandleId::new(2123),
            body: PluginBody::VideoRoom(VideoRoomPluginBody::Start(VideoRoomPluginStart {})),
            jsep: Some(Jsep {
                kind: JsepType::Offer,
                sdp: "v=0".to_string(),
                trickle: None,
            }),
        });

        assert_eq_json!(
            plugin_message,
            {
                "janus": "message",
                "handle_id": 2123,
                "session_id": 234,
                "body": {
                    "request":"start"
                },
                "jsep": {
                    "type": "offer",
                    "sdp": "v=0"
                }
            }
        );
    }

    #[test]
    fn test_join_sub() {
        let plugin_message = JanusRequest::PluginMessage(PluginMessage {
            session_id: SessionId::new(234),
            handle_id: HandleId::new(2123),
            body: PluginBody::VideoRoom(VideoRoomPluginBody::Join(
                VideoRoomPluginJoin::Subscriber(
                    VideoRoomPluginJoinSubscriber::builder(5.into(), 1.into()).build(),
                ),
            )),
            jsep: Some(Jsep {
                kind: JsepType::Offer,
                sdp: "v=0".to_string(),
                trickle: None,
            }),
        });

        assert_eq_json!(
            plugin_message,
            {
                "janus": "message",
                "handle_id": 2123,
                "session_id": 234,
                "body": {
                    "request": "join",
                    "ptype": "subscriber",
                    "room": 5,
                    "feed": 1
                },
                "jsep": {
                    "type": "offer",
                    "sdp": "v=0"
                }
            }
        );
    }

    #[test]
    fn test_video_room_plugin_join_subscriber_builder() {
        let request = VideoRoomPluginJoinSubscriber::builder(5.into(), 1.into())
            .private_id(Some(42))
            .close_pc(Some(true))
            .audio(Some(true))
            .video(Some(true))
            .data(Some(false))
            .offer_audio(Some(true))
            .offer_video(Some(true))
            .offer_data(Some(false))
            .substream(Some(4))
            .temporal_layer(Some(4))
            .spatial_layer(Some(4))
            .temporal(Some(4))
            .build();

        assert_eq!(request.private_id, Some(42));
        assert_eq!(request.close_pc, Some(true));
        assert_eq!(request.audio, Some(true));
        assert_eq!(request.video, Some(true));
        assert_eq!(request.data, Some(false));
        assert_eq!(request.offer_audio, Some(true));
        assert_eq!(request.offer_video, Some(true));
        assert_eq!(request.offer_data, Some(false));
        assert_eq!(request.substream, Some(2));
        assert_eq!(request.temporal_layer, Some(2));
        assert_eq!(request.spatial_layer, Some(2));
        assert_eq!(request.temporal, Some(2));
    }

    #[test]
    fn test_list() {
        let plugin_message = JanusRequest::PluginMessage(PluginMessage {
            session_id: SessionId::new(234),
            handle_id: HandleId::new(2123),
            body: PluginBody::VideoRoom(VideoRoomPluginBody::ListRooms(VideoRoomPluginListRooms)),
            jsep: None,
        });

        assert_eq_json!(
            plugin_message,
            {
                "janus": "message",
                "handle_id": 2123,
                "session_id": 234,
                "body": {
                  "request": "list"
                }
            }
        );
    }

    #[test]
    fn test_join_pub() {
        let plugin_message = JanusRequest::PluginMessage(PluginMessage {
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

        assert_eq_json!(
            plugin_message,
            {
                "janus": "message",
                "handle_id": 2123,
                "session_id": 234,
                "body": {
                  "request": "join",
                  "ptype": "publisher",
                  "room": 5,
                  "id": 1
                },
                "jsep": {
                    "type": "offer",
                    "sdp": "v=0"
                }
            }
        );
    }
    #[test]
    fn test_destroy() {
        let plugin_message = JanusRequest::PluginMessage(PluginMessage {
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

        assert_eq_json!(
            plugin_message,
            {
                "janus": "message",
                "handle_id": 2123,
                "session_id": 234,
                "body": {
                  "request": "destroy",
                  "room": 1
                },
                "jsep": {
                    "type": "offer",
                    "sdp": "v=0"
                }
            }
        );
    }

    #[test]
    fn test_full_room_config() {
        let plugin_message = JanusRequest::PluginMessage(PluginMessage {
            session_id: SessionId::new(123),
            handle_id: HandleId::new(234),
            body: PluginBody::VideoRoom(VideoRoomPluginBody::Create(VideoRoomPluginCreate {
                description: "TestRoom|StreamType".into(),
                publishers: Some(1),
                videoorient_ext: Some(false),
                videocodec: vec![VideoCodec::Av1, VideoCodec::Vp8],
                audiocodec: vec![AudioCodec::G722],
                ..Default::default()
            })),
            jsep: None,
        });

        assert_eq_json!(
            plugin_message,
            {
                "janus": "message",
                "handle_id": 234,
                "session_id": 123,
                "body": {
                    "request": "create",
                    "description": "TestRoom|StreamType",
                    "publishers": 1,
                    "videoorient_ext": false,
                    "audiocodec": "g722",
                    "videocodec": "av1,vp8"
                }
            }
        );
    }
}
