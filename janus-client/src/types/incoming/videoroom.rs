//! Datatypes for the VideoRoom plugin

use crate::error::JanusPluginError;
use crate::types::{AudioCodec, RoomId, VideoCodec};
use crate::{
    error::{self, JanusError},
    PluginData,
};
use serde::{
    self,
    de::{self, Visitor},
    Deserialize, Deserializer,
};
use std::marker::PhantomData as Phantom;
use std::path::PathBuf;
use std::{convert::TryFrom, iter::FromIterator};
use std::{
    fmt::{self, Display},
    str::FromStr,
};

/// Plugin response types
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "videoroom")]
pub enum VideoRoomPluginData {
    #[serde(rename = "created")]
    Created(VideoRoomPluginDataCreated),
    #[serde(rename = "success")]
    Success(VideoRoomPluginDataSuccess),
    #[serde(rename = "joined")]
    Joined(VideoRoomPluginDataJoined),
    #[serde(rename = "event")]
    Event(VideoRoomPluginEvent),
    #[serde(rename = "started")]
    Started(VideoRoomPluginDataStarted),
    #[serde(rename = "destroyed")]
    Destroyed(VideoRoomPluginDataDestroyed),
    #[serde(rename = "attached")]
    Attached(VideoRoomPluginDataAttached),
}

/// A room
#[derive(Debug, Clone, Deserialize)]
pub struct Room {
    /// unique numeric ID
    #[serde(rename = "room")]
    id: RoomId,
    /// <Name of the room>
    description: String,
    /// true|false, whether a PIN is required to join this room
    pin_required: bool,
    /// how many publishers can actually publish via WebRTC at the same time
    max_publishers: u64,
    /// bitrate cap that should be forced (via REMB) on all publishers by default, in bit/s
    bitrate: u64,
    /// true|false, whether the above cap should act as a limit to dynamic bitrate changes by publishers
    #[serde(default)]
    bitrate_cap: bool,
    /// how often a keyframe request is sent via PLI/FIR to active publishers
    fir_freq: u64,
    /// todo ?
    require_pvtid: bool,
    /// todo ?
    require_e2ee: bool,
    /// todo ?
    notify_joining: bool,
    /// <comma separated list of allowed audio codecs>
    #[serde(deserialize_with = "comma_separated")]
    audiocodec: Vec<AudioCodec>,
    #[serde(deserialize_with = "comma_separated")]
    /// <comma separated list of allowed video codecs>
    videocodec: Vec<VideoCodec>,
    /// todo ?
    #[serde(default)]
    opus_fec: bool,
    /// todo ?
    #[serde(default)]
    video_svc: bool,
    /// true|false, whether the room is being recorded
    record: bool,
    /// <if recording, the path where the .mjr files are being saved>
    #[serde(default)]
    rec_dir: Option<PathBuf>,
    /// true|false, whether the room recording state can only be changed providing the secret
    lock_record: bool,
    /// count of the participants (publishers, active or not; not subscribers)
    num_participants: u64,
    /// todo ?
    audiolevel_ext: bool,
    /// todo ?
    audiolevel_event: bool,
    /// todo ?
    #[serde(default)]
    audio_active_packets: u64,
    /// todo ?
    #[serde(default)]
    audio_level_average: u64,
    /// todo ?
    videoorient_ext: bool,
    /// todo ?
    playoutdelay_ext: bool,
    /// todo ?
    transport_wide_cc_ext: bool,
}

impl Room {
    pub fn description(&self) -> &String {
        &self.description
    }
}

impl std::default::Default for Room {
    fn default() -> Self {
        Self {
            id: 0.into(),
            description: "".to_owned(),
            pin_required: false,
            max_publishers: 3,
            bitrate: 0,
            bitrate_cap: false,
            fir_freq: 0,
            require_pvtid: false,
            require_e2ee: false,
            notify_joining: false,
            audiocodec: vec![AudioCodec::Opus],
            videocodec: vec![VideoCodec::Vp8],
            record: false,
            rec_dir: None,
            lock_record: false,
            num_participants: 0,
            audiolevel_ext: true,
            audiolevel_event: false,
            videoorient_ext: true,
            playoutdelay_ext: true,
            transport_wide_cc_ext: true,
            opus_fec: false,
            video_svc: false,
            audio_active_packets: 0,
            audio_level_average: 0,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
enum OkEnum {
    #[serde(rename = "ok")]
    Ok,
}

// Created response type
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum VideoRoomPluginDataCreated {
    Ok { room: RoomId, permanent: bool },
    Err(JanusError),
}

impl TryFrom<PluginData> for VideoRoomPluginDataCreated {
    type Error = error::Error;

    fn try_from(value: PluginData) -> Result<Self, Self::Error> {
        match value {
            PluginData::VideoRoom(VideoRoomPluginData::Created(e)) => Ok(e),
            _ => Err(error::Error::InvalidResponse),
        }
    }
}

/// Success reponse type
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum VideoRoomPluginDataSuccess {
    List { list: Vec<Room> },
}

// Todo split this up to provide a try into List directly.
impl TryFrom<PluginData> for VideoRoomPluginDataSuccess {
    type Error = error::Error;

    fn try_from(value: PluginData) -> Result<Self, Self::Error> {
        match value {
            PluginData::VideoRoom(VideoRoomPluginData::Success(e)) => Ok(e),
            _ => Err(error::Error::InvalidResponse),
        }
    }
}

/// Joined response type
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum VideoRoomPluginDataJoined {
    Ok {
        room: RoomId,
        description: String,
        id: u64,
        private_id: u64,
        publishers: Vec<u64>,
    },
    Err(JanusError),
}

impl TryFrom<PluginData> for VideoRoomPluginDataJoined {
    type Error = error::Error;

    fn try_from(value: PluginData) -> Result<Self, Self::Error> {
        match value {
            PluginData::VideoRoom(VideoRoomPluginData::Joined(e)) => Ok(e),
            _ => Err(error::Error::InvalidResponse),
        }
    }
}

/// Event types, normally are received via the "incoming channel"
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum VideoRoomPluginEvent {
    Configured(VideoRoomPluginEventConfigured),
    Leaving(VideoRoomPluginEventLeaving),
    Started(VideoRoomPluginEventStarted),
    /// Errors returned for a specific plugin.
    /// E.g. No Such Feed errors
    Error(JanusPluginError),
}

#[derive(Debug, Clone, Deserialize)]
pub struct VideoRoomPluginEventConfigured {
    configured: OkEnum,
    room: RoomId,
    audio_codec: Option<AudioCodec>,
    video_codec: Option<VideoCodec>,
    // Here might be more missing, depending on what we configured
}

impl TryFrom<PluginData> for VideoRoomPluginEventConfigured {
    type Error = error::Error;

    fn try_from(value: PluginData) -> Result<Self, Self::Error> {
        match value {
            PluginData::VideoRoom(VideoRoomPluginData::Event(
                VideoRoomPluginEvent::Configured(e),
            )) => Ok(e),
            _ => Err(error::Error::InvalidResponse),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct VideoRoomPluginEventLeaving {
    leaving: OkEnum,
}

impl TryFrom<PluginData> for VideoRoomPluginEventLeaving {
    type Error = error::Error;

    fn try_from(value: PluginData) -> Result<Self, Self::Error> {
        match value {
            PluginData::VideoRoom(VideoRoomPluginData::Event(VideoRoomPluginEvent::Leaving(e))) => {
                Ok(e)
            }
            _ => Err(error::Error::InvalidResponse),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct VideoRoomPluginEventStarted {
    room: RoomId,
    started: OkEnum,
}

impl TryFrom<PluginData> for VideoRoomPluginEventStarted {
    type Error = error::Error;

    fn try_from(value: PluginData) -> Result<Self, Self::Error> {
        match value {
            PluginData::VideoRoom(VideoRoomPluginData::Event(VideoRoomPluginEvent::Started(e))) => {
                Ok(e)
            }
            _ => Err(error::Error::InvalidResponse),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct VideoRoomPluginDataStarted(OkEnum);

impl TryFrom<PluginData> for VideoRoomPluginDataStarted {
    type Error = error::Error;

    fn try_from(value: PluginData) -> Result<Self, Self::Error> {
        match value {
            PluginData::VideoRoom(VideoRoomPluginData::Started(e)) => Ok(e),
            _ => Err(error::Error::InvalidResponse),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct VideoRoomPluginDataDestroyed {
    room: RoomId,
    permanent: Option<bool>,
}

impl TryFrom<PluginData> for VideoRoomPluginDataDestroyed {
    type Error = error::Error;

    fn try_from(value: PluginData) -> Result<Self, Self::Error> {
        match value {
            PluginData::VideoRoom(VideoRoomPluginData::Destroyed(e)) => Ok(e),
            _ => Err(error::Error::InvalidResponse),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct VideoRoomPluginDataAttached {
    room: RoomId,
    display: Option<String>,
}

impl TryFrom<PluginData> for VideoRoomPluginDataAttached {
    type Error = error::Error;

    fn try_from(value: PluginData) -> Result<Self, Self::Error> {
        match value {
            PluginData::VideoRoom(VideoRoomPluginData::Attached(e)) => Ok(e),
            _ => Err(error::Error::InvalidResponse),
        }
    }
}

fn comma_separated<'de, V, T, D>(deserializer: D) -> Result<V, D::Error>
where
    V: FromIterator<T>,
    T: FromStr,
    T::Err: Display,
    D: Deserializer<'de>,
{
    struct CommaSeparated<V, T>(Phantom<V>, Phantom<T>);

    impl<'de, V, T> Visitor<'de> for CommaSeparated<V, T>
    where
        V: FromIterator<T>,
        T: FromStr,
        T::Err: Display,
    {
        type Value = V;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("string containing comma-separated elements")
        }

        fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let iter = s.split(',').map(FromStr::from_str);
            Result::from_iter(iter).map_err(de::Error::custom)
        }
    }

    let visitor = CommaSeparated(Phantom, Phantom);
    deserializer.deserialize_str(visitor)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        types::incoming::Event, types::incoming::PluginSuccess, types::Success,
        types::TransactionId, HandleId, JanusMessage, PluginData, SessionId,
    };

    #[test]
    fn parse_room_create() {
        let json = r#"{
        "janus": "success",
        "session_id": 1181318522471683,
        "transaction": "2",
        "sender": 7519437590873898,
        "plugindata": {
            "plugin": "janus.plugin.videoroom",
            "data": {
                "videoroom": "created",
                "room": 4720732281562341,
                "permanent": false
            }
        }
        }"#;
        println!("{}", json);

        let parsed_result: JanusMessage = serde_json::from_str(json).unwrap();
        match parsed_result {
            JanusMessage::Success(Success::Plugin(PluginSuccess {
                sender,
                transaction,
                plugindata,
                jsep: None,
                session_id,
            })) => {
                assert!(sender.unwrap() == HandleId::new(7519437590873898));
                assert!(session_id.unwrap() == SessionId::new(1181318522471683));
                assert!(transaction == TransactionId("2".into()));
                if let PluginData::VideoRoom(VideoRoomPluginData::Created(
                    VideoRoomPluginDataCreated::Ok { room, permanent },
                )) = plugindata
                {
                    assert!(room == 4720732281562341.into());
                    assert!(permanent == false);
                } else {
                    assert!(false)
                }
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn parse_missing_transaction_id() {
        let json = r#"{
        "janus": "event",
        "session_id": 1181318522471683,
        "sender": 7519437590873898,
        "plugindata": {
            "plugin": "janus.plugin.videoroom",
            "data": {
                "videoroom": "created",
                "room": 4720732281562341,
                "permanent": false
            }
        },
        "jsep": {"type": "offer", "sdp": "v=0.."}
        }"#;
        println!("{}", json);

        let parsed_result: JanusMessage = serde_json::from_str(json).unwrap();
        dbg!(&parsed_result);
        match parsed_result {
            JanusMessage::Event(Event {
                sender,
                transaction,
                session_id,
                ..
            }) => {
                assert!(sender == HandleId::new(7519437590873898));
                assert!(session_id == SessionId::new(1181318522471683));
                assert!(transaction == None);
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn parse_event_started() {
        let json = r#"{
               "janus": "event",
               "session_id": 3736408189546184,
               "transaction": "16",
               "sender": 6061082733923198,
               "plugindata": {
                  "plugin": "janus.plugin.videoroom",
                  "data": {
                     "videoroom": "event",
                     "room": 5156409674383772,
                     "started": "ok"
                  }
               }
        }"#;
        println!("{}", json);

        let parsed_result: JanusMessage = serde_json::from_str(json).unwrap();
        dbg!(&parsed_result);
        match parsed_result {
            JanusMessage::Event(Event {
                sender,
                transaction,
                session_id,
                plugindata:
                    PluginData::VideoRoom(VideoRoomPluginData::Event(VideoRoomPluginEvent::Started(
                        VideoRoomPluginEventStarted { .. },
                    ))),
                ..
            }) => {
                assert!(sender == HandleId::new(6061082733923198));
                assert!(session_id == SessionId::new(3736408189546184));
                assert!(transaction == Some(16.into()));
            }
            _ => assert!(false),
        }
    }
    #[test]
    fn parse_no_feed() {
        let json = r#"{
            "janus": "event",
            "session_id": 5722050567499805,
            "transaction": "124",
            "sender": 4366965359665307,
            "plugindata": {
               "plugin": "janus.plugin.videoroom",
               "data": {
                  "videoroom": "event",
                  "error_code": 428,
                  "error": "No such feed (1)"
               }
            }
         }"#;
        println!("{}", json);

        let parsed_result: JanusMessage = serde_json::from_str(json).unwrap();
        dbg!(&parsed_result);
        match parsed_result {
            JanusMessage::Event(Event {
                sender,
                transaction,
                session_id,
                plugindata:
                    PluginData::VideoRoom(VideoRoomPluginData::Event(VideoRoomPluginEvent::Error(e))),
                ..
            }) => {
                assert!(sender == HandleId::new(4366965359665307));
                assert!(session_id == SessionId::new(5722050567499805));
                assert!(transaction == Some(124.into()));
                assert!(e.error_code() == error::JanusInternalError::VideoroomErrorNoSuchFeed);
                assert!(e.reason() == "No such feed (1)");
            }
            _ => assert!(false),
        }
    }
    #[test]
    fn parse_screen_response() {
        // enum VideoRoomPluginDataEvent
        let json = r#"{
            "janus": "event",
            "session_id": 4618053420922813,
            "transaction": "5",
            "sender": 6977232445950259,
            "plugindata": {
                "plugin": "janus.plugin.videoroom",
                "data": {
                    "videoroom": "event",
                    "room": 1132341855884658,
                    "configured": "ok",
                    "video_codec": "vp8"
                }
            },
            "jsep": {
                "type": "answer",
                "sdp": "v=0\r\no=mozilla...THIS_IS_SDPARTA-88.0 1621934037162508 1 IN IP4 192.168.0.157\r\ns=VideoRoom 1132341855884658\r\nt=0 0\r\na=group:BUNDLE 0\r\na=msid-semantic: WMS janus\r\nm=video 9 UDP/TLS/RTP/SAVPF 120 124\r\nc=IN IP4 192.168.0.157\r\nb=TIAS:64000\r\na=recvonly\r\na=mid:0\r\na=rtcp-mux\r\na=ice-ufrag:tzKY\r\na=ice-pwd:AMOgviPceqysnvGfi+5/d2\r\na=ice-options:trickle\r\na=fingerprint:sha-256 2D:50:B4:8E:4D:A7:57:62:8A:B3:A1:CC:A3:46:A0:C6:FA:06:CC:39:EC:3F:A2:54:C7:84:8B:2E:81:BF:C3:CB\r\na=setup:active\r\na=rtpmap:120 VP8/90000\r\na=rtcp-fb:120 ccm fir\r\na=rtcp-fb:120 nack\r\na=rtcp-fb:120 nack pli\r\na=rtcp-fb:120 goog-remb\r\na=rtcp-fb:120 transport-cc\r\na=extmap:3 urn:ietf:params:rtp-hdrext:sdes:mid\r\na=extmap:6/inactive http://www.webrtc.org/experiments/rtp-hdrext/playout-delay\r\na=extmap:7 http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01\r\na=fmtp:120 max-fs=12288;max-fr=60\r\na=rtpmap:124 rtx/90000\r\na=fmtp:124 apt=120\r\na=msid:janus janusv0\r\na=ssrc:4045672590 cname:janus\r\na=ssrc:4045672590 msid:janus janusv0\r\na=ssrc:4045672590 mslabel:janus\r\na=ssrc:4045672590 label:janusv0\r\na=ssrc:2597748152 cname:janus\r\na=ssrc:2597748152 msid:janus janusv0\r\na=ssrc:2597748152 mslabel:janus\r\na=ssrc:2597748152 label:janusv0\r\na=candidate:1 1 udp 2015363839 192.168.0.157 33995 typ host\r\na=candidate:2 1 udp 2015364095 10.0.50.30 39626 typ host\r\na=end-of-candidates\r\n"
            }
            }"#;
        println!("{}", json);

        let parsed_result: JanusMessage = serde_json::from_str(json).unwrap();
        dbg!(&parsed_result);
        match parsed_result {
            JanusMessage::Event(Event {
                sender,
                transaction,
                session_id,
                plugindata:
                    PluginData::VideoRoom(VideoRoomPluginData::Event(VideoRoomPluginEvent::Configured(
                        VideoRoomPluginEventConfigured {
                            configured,
                            video_codec,
                            ..
                        },
                    ))),
                ..
            }) => {
                assert!(session_id == SessionId::new(4618053420922813));
                assert!(transaction == Some(5.into()));
                assert!(sender == HandleId::new(6977232445950259));
                assert!(configured == OkEnum::Ok);
                assert!(video_codec == Some(VideoCodec::Vp8))
            }
            _ => assert!(false),
        }
    }
    #[test]
    fn parse_destroyed() {
        // enum VideoRoomPluginDataEvent
        let json = r#"{
            "janus": "success",
            "session_id": 8768646295727490,
            "transaction": "106",
            "sender": 7265638106357492,
            "plugindata": {
               "plugin": "janus.plugin.videoroom",
               "data": {
                  "videoroom": "destroyed",
                  "room": 2987623648982456,
                  "permanent": false
               }
            }
         }"#;
        let json2 = r#"{
            "janus": "event",
            "session_id": 3771726839576225,
            "sender": 779175911287262,
            "plugindata": {
               "plugin": "janus.plugin.videoroom",
               "data": {
                  "videoroom": "destroyed",
                  "room": 1605851680987154
               }
            }
         }"#;
        println!("{}", json);

        let parsed_result: JanusMessage = serde_json::from_str(json).unwrap();
        dbg!(&parsed_result);
        match parsed_result {
            JanusMessage::Success(Success::Plugin(PluginSuccess {
                sender,
                transaction,
                session_id,
                plugindata:
                    PluginData::VideoRoom(VideoRoomPluginData::Destroyed(
                        VideoRoomPluginDataDestroyed {
                            room,
                            permanent: Some(false),
                        },
                    )),
                ..
            })) => {
                assert!(session_id == Some(SessionId::new(8768646295727490)));
                assert!(transaction == 106.into());
                assert!(sender == Some(HandleId::new(7265638106357492)));
                assert!(room == 2987623648982456.into());
            }
            _ => assert!(false, "Got no Destroyed response"),
        }
        let parsed_result: JanusMessage = serde_json::from_str(json2).unwrap();
        dbg!(&parsed_result);
        match parsed_result {
            JanusMessage::Event(Event {
                sender,
                plugindata:
                    PluginData::VideoRoom(VideoRoomPluginData::Destroyed(
                        VideoRoomPluginDataDestroyed {
                            room,
                            permanent: None,
                        },
                    )),
                ..
            }) => {
                assert!(sender == HandleId::new(779175911287262));
                assert!(
                    room == 1605851680987154.into(),
                    "RoomId wrong for Destroyed Event"
                );
            }
            _ => assert!(false, "Got no Destroyed Event"),
        }
    }

    #[test]
    fn parse_room_list() {
        let json = r#"{
            "janus": "success",
            "session_id": 6306085741004171,
            "transaction": "4",
            "sender": 8590982532148274,
            "plugindata": {
                "plugin": "janus.plugin.videoroom",
                "data": {
                    "videoroom": "success",
                    "list": [
                        {
                            "room": 7610511204322687,
                            "description": "Testroom1",
                            "pin_required": false,
                            "max_publishers": 3,
                            "bitrate": 0,
                            "fir_freq": 0,
                            "require_pvtid": false,
                            "require_e2ee": false,
                            "notify_joining": false,
                            "audiocodec": "opus",
                            "videocodec": "vp8",
                            "record": false,
                            "lock_record": false,
                            "num_participants": 0,
                            "audiolevel_ext": true,
                            "audiolevel_event": false,
                            "videoorient_ext": true,
                            "playoutdelay_ext": true,
                            "transport_wide_cc_ext": true
                        },
                        {
                            "room": 5678,
                            "description": "VP9-SVC Demo Room",
                            "pin_required": false,
                            "max_publishers": 6,
                            "bitrate": 512000,
                            "fir_freq": 10,
                            "require_pvtid": false,
                            "require_e2ee": false,
                            "notify_joining": false,
                            "audiocodec": "opus",
                            "videocodec": "vp9",
                            "video_svc": true,
                            "record": false,
                            "lock_record": false,
                            "num_participants": 0,
                            "audiolevel_ext": true,
                            "audiolevel_event": false,
                            "videoorient_ext": true,
                            "playoutdelay_ext": true,
                            "transport_wide_cc_ext": true
                         }
                    ]
                }
            }
        }"#;

        let parsed_result: JanusMessage = serde_json::from_str(json).unwrap();
        if let JanusMessage::Success(Success::Plugin(PluginSuccess {
            sender,
            transaction,
            plugindata,
            jsep: None,
            session_id,
        })) = parsed_result
        {
            assert!(sender.unwrap() == HandleId::new(8590982532148274));
            assert!(session_id.unwrap() == SessionId::new(6306085741004171));
            assert!(transaction == TransactionId("4".into()));
            if let PluginData::VideoRoom(VideoRoomPluginData::Success(
                VideoRoomPluginDataSuccess::List { list },
            )) = plugindata
            {
                assert!(list.len() == 2);
                assert!(list[0].id == 7610511204322687.into());
                assert!(list[1].id == 5678.into());
                assert!(list[1].video_svc == true);
            } else {
                panic!()
            }
        } else {
            panic!()
        }
    }
}
