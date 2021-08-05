use anyhow::{Context, Result};
use controller::prelude::*;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct JanusMcuConfig {
    pub connections: Vec<JanusRabbitMqConnection>,

    /// Max bitrate allowed for `video` media sessions
    #[serde(default = "default_max_video_bitrate")]
    pub max_video_bitrate: u64,

    /// Max bitrate allowed for `screen` media sessions
    #[serde(default = "default_max_screen_bitrate")]
    pub max_screen_bitrate: u64,
}

impl JanusMcuConfig {
    pub fn extract(settings: &controller::settings::Settings) -> Result<Self> {
        let value = settings
            .extensions
            .get("room_server")
            .cloned()
            .context("missing room server")?;

        value
            .try_into()
            .context("failed to deserialize room_server config")
    }
}

/// Take the settings from your janus rabbit mq transport configuration.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct JanusRabbitMqConnection {
    #[serde(default = "default_to_janus_routing_key")]
    pub to_janus_routing_key: String,
    #[serde(default = "default_janus_exchange")]
    pub janus_exchange: String,
    #[serde(default = "default_from_janus_routing_key")]
    pub from_janus_routing_key: String,
}

const fn default_max_video_bitrate() -> u64 {
    // 8kB/s
    64000
}

const fn default_max_screen_bitrate() -> u64 {
    // 1 MB/s
    8_000_000
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
