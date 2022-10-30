// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! Outgoing EchoTest plugin datatypes
//!
use super::{AudioCodec, VideoCodec};
use crate::{incoming, outgoing::PluginBody, PluginRequest};
use serde::{self, Serialize};
use std::path::PathBuf;

/// Plugin request body for the echotest plugin
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "request")]
pub enum EchoPluginBody {
    #[serde(rename = "unnamed")]
    Unnamed(EchoPluginUnnamed),
}

/// Unnamed call
///
/// Echoes back.
/// See <https://janus.conf.meetecho.com/docs/echotest.html> for more information
#[derive(Debug, Clone, Serialize, Default)]
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

impl PluginRequest for EchoPluginUnnamed {
    type PluginResponse = incoming::EchoPluginDataEvent;
    const IS_ASYNC: bool = true;
}

impl From<EchoPluginUnnamed> for PluginBody {
    fn from(value: EchoPluginUnnamed) -> Self {
        PluginBody::EchoTest(EchoPluginBody::Unnamed(value))
    }
}
