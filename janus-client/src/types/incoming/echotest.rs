//! Datatypes for the EchoTest plugin

use serde::{self, Deserialize};
use crate::{error, PluginData};
use super::{AudioCodec, JanusInternalError, VideoCodec};
use std::{convert::TryFrom, path::PathBuf};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct EchoPluginUnnamed {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audiocodec: Option<AudioCodec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub videocodec: Option<VideoCodec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub videoprofile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub substream: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal: Option<u8>,
}

impl TryFrom<PluginData> for EchoPluginUnnamed {
    type Error = error::Error;

    fn try_from(value: PluginData) -> Result<Self, Self::Error> {
        match value {
            PluginData::EchoTest(EchoPluginData::Unnamed(e)) => Ok(e),
            _ => Err(error::Error::InvalidResponse),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "echotest")]
pub enum EchoPluginData {
    #[serde(rename = "event")]
    Event(EchoPluginDataEvent),
    #[serde(rename = "echotest")]
    Unnamed(EchoPluginUnnamed),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum EchoPluginDataEvent {
    Ok {
        result: String,
    },
    Err {
        error: String,
        error_code: JanusInternalError,
    },
}

impl TryFrom<PluginData> for EchoPluginDataEvent {
    type Error = error::Error;

    fn try_from(value: PluginData) -> Result<Self, Self::Error> {
        match value {
            PluginData::EchoTest(EchoPluginData::Event(e)) => Ok(e),
            _ => Err(error::Error::InvalidResponse),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::types::{
        incoming::{Event, JanusMessage, PluginData},
        HandleId, SessionId, TransactionId,
    };

    #[test]
    #[cfg(feature = "echotest")]
    fn parse_echo_test() {
        let json = r#"{
            "janus" : "event",
            "sender" : 1815153248,
            "session_id": 1234,
            "transaction" : "sBJNyUhH6Vc6",
            "plugindata" : {
                    "plugin": "janus.plugin.echotest",
                    "data" : {
                            "echotest" : "event",
                            "result" : "ok"
                    }
            }
        }"#;
        let parsed_result: JanusMessage = serde_json::from_str(json).unwrap();
        match parsed_result {
            JanusMessage::Event(Event {
                sender,
                transaction,
                plugindata,
                jsep: None,
                session_id,
            }) => {
                assert!(sender == HandleId::new(1815153248));
                assert!(transaction.unwrap() == TransactionId("sBJNyUhH6Vc6".into()));
                assert!(session_id == SessionId::new(1234));
                match plugindata {
                    PluginData::EchoTest(data) => match data {
                        EchoPluginData::Event(EchoPluginDataEvent::Ok { result }) => {
                            assert!(result == "ok");
                        }
                        _ => {
                            assert!(false)
                        }
                    },
                    _ => {
                        assert!(false)
                    }
                }
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn parse_echo_test_error() {
        let json = r#"{
            "janus": "event",
            "session_id": 280835257881068,
            "transaction": "2",
            "sender": 1739642080530886,
            "plugindata": {
               "plugin": "janus.plugin.echotest",
               "data": {
                  "echotest": "event",
                  "error_code": 413,
                  "error": "Invalid value (video should be a boolean)"
               }
            }
         }"#;

        let parsed_result: JanusMessage = serde_json::from_str(json).unwrap();
        match parsed_result {
            JanusMessage::Event(Event {
                sender,
                transaction,
                plugindata,
                jsep: None,
                session_id,
            }) => {
                assert!(sender == HandleId::new(1739642080530886));
                assert!(session_id == SessionId::new(280835257881068));
                assert!(transaction.unwrap() == TransactionId("2".into()));
                match plugindata {
                    PluginData::EchoTest(data) => match data {
                        EchoPluginData::Event(EchoPluginDataEvent::Err { error_code, .. }) => {
                            assert!(error_code == JanusInternalError::EchotestErrorInvalidElement);
                        }
                        _ => {
                            assert!(false)
                        }
                    },
                    _ => {
                        assert!(false)
                    }
                }
            }
            _ => assert!(false),
        }
    }
}
