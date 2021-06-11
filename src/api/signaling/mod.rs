use crate::api::signaling::local::Room;
use crate::api::signaling::ws_modules::room::RoomControl;
use crate::modules::http::ws::{Echo, WebSocketHttpModule};
use crate::modules::ApplicationBuilder;
use crate::settings;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

mod local;
mod mcu;
mod media;
mod ws_modules;

pub use mcu::JanusMcu;

#[derive(Debug, Serialize, Deserialize, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct ParticipantId(Uuid);

impl ParticipantId {
    #[cfg(test)]
    pub(crate) fn nil() -> Self {
        Self(Uuid::nil())
    }

    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for ParticipantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

pub async fn attach(
    application: &mut ApplicationBuilder,
    room_server: settings::JanusMcuConfig,
    rabbit_mq: settings::RabbitMqConfig,
) -> Result<()> {
    let rabbitmq_connection =
        lapin::Connection::connect(&rabbit_mq.url, lapin::ConnectionProperties::default())
            .await
            .context("failed to connect to rabbitmq")?;

    let mcu_channel = rabbitmq_connection
        .create_channel()
        .await
        .context("Could not create rabbit mq channel for MCU")?;

    let mcu = Arc::new(JanusMcu::connect(room_server, mcu_channel).await?);
    mcu.start()?;

    let room = Arc::new(RwLock::new(Room { members: vec![] }));

    let mut signaling = WebSocketHttpModule::new("/signaling", &["k3k-signaling-json-v1"]);

    signaling.add_module::<Echo>(());
    signaling.add_module::<RoomControl>((mcu, room));

    application.add_http_module(signaling);

    Ok(())
}
