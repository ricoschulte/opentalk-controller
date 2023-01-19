// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! Datatypes for the EchoTest plugin

use super::{AudioCodec, VideoCodec};
use crate::error::JanusPluginError;
use crate::{error, PluginData};
use serde::{self, Deserialize};
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
    Ok { result: String },
    Error(JanusPluginError),
}

impl TryFrom<PluginData> for EchoPluginDataEvent {
    type Error = error::Error;

    fn try_from(value: PluginData) -> Result<Self, error::Error> {
        match value {
            PluginData::EchoTest(EchoPluginData::Event(e)) => Ok(e),
            _ => Err(error::Error::InvalidResponse),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::error::JanusInternalError;
    use crate::types::{
        incoming::{Event, JanusMessage, PluginData},
        HandleId, SessionId, TransactionId,
    };
    use pretty_assertions::assert_eq;

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
                plugindata:
                    PluginData::EchoTest(EchoPluginData::Event(EchoPluginDataEvent::Ok { result })),
                jsep: None,
                session_id,
            }) => {
                assert_eq!(sender, HandleId::new(1815153248));
                assert_eq!(transaction.unwrap(), TransactionId("sBJNyUhH6Vc6".into()));
                assert_eq!(session_id, SessionId::new(1234));
                assert_eq!(result, "ok");
            }
            _ => panic!(),
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
                plugindata:
                    PluginData::EchoTest(EchoPluginData::Event(EchoPluginDataEvent::Error(error))),
                jsep: None,
                session_id,
            }) => {
                assert_eq!(sender, HandleId::new(1739642080530886));
                assert_eq!(session_id, SessionId::new(280835257881068));
                assert_eq!(transaction.unwrap(), TransactionId("2".into()));
                assert_eq!(
                    error.error_code(),
                    JanusInternalError::EchotestErrorInvalidElement
                );
            }
            _ => panic!(),
        }
    }
}
