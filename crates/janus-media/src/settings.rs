use anyhow::{Context, Result};
use controller::prelude::*;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct JanusMcuConfig {
    pub connections: Vec<Connection>,

    /// Max bitrate allowed for `video` media sessions
    #[serde(default = "default_max_video_bitrate")]
    pub max_video_bitrate: u64,

    /// Max bitrate allowed for `screen` media sessions
    #[serde(default = "default_max_screen_bitrate")]
    pub max_screen_bitrate: u64,

    /// Number of packets with with given `speaker_focus_level`
    /// needed to detect a speaking participant.
    ///
    /// Default: 50 packets (1 second of audio)
    #[serde(default = "default_speaker_focus_packets")]
    pub speaker_focus_packets: i64,

    /// Average value of audio level needed per packet.
    ///
    /// min: 127 (muted)  
    /// max: 0   (loud)  
    /// default: 50  
    #[serde(default = "default_speaker_focus_level")]
    pub speaker_focus_level: i64,
}

impl JanusMcuConfig {
    pub fn extract(settings: &controller::settings::Settings) -> Result<Self> {
        let value = settings
            .extensions
            .get("room_server")
            .cloned()
            .context("missing room server")?;

        value
            .try_deserialize()
            .context("failed to deserialize room_server config")
    }
}

/// Take the settings from your janus rabbit mq transport configuration.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Connection {
    #[serde(default = "default_to_janus_routing_key")]
    pub to_routing_key: String,
    #[serde(default = "default_janus_exchange")]
    pub exchange: String,
    #[serde(default = "default_from_janus_routing_key")]
    pub from_routing_key: String,
}

const fn default_max_video_bitrate() -> u64 {
    // 1.5 Mbit/s
    1_500_000
}

const fn default_max_screen_bitrate() -> u64 {
    // 1 MB/s
    8_000_000
}

const fn default_speaker_focus_packets() -> i64 {
    50
}

const fn default_speaker_focus_level() -> i64 {
    50
}

fn default_to_janus_routing_key() -> String {
    "to-janus".to_owned()
}

fn default_janus_exchange() -> String {
    "janus-exchange".to_owned()
}

fn default_from_janus_routing_key() -> String {
    "from-janus".to_owned()
}
