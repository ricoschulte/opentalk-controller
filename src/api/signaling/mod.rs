use crate::api::signaling::local::Room;
use crate::api::signaling::ws_modules::room::RoomControl;
use crate::modules::ApplicationBuilder;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;
use ws::{Echo, SignalingHttpModule};

mod local;
mod mcu;
mod media;
mod redis_state;
mod ws;
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
    shutdown_sig: broadcast::Receiver<()>,
    redis: &redis::Client,
    mcu: &Arc<JanusMcu>,
) -> Result<()> {
    let msg_sender = redis_state::start_keyspace_notification_loop(shutdown_sig, redis).await?;

    let room = Arc::new(RwLock::new(Room { members: vec![] }));

    let mut signaling = SignalingHttpModule::new();

    signaling.add_module::<Echo>(());
    signaling.add_module::<RoomControl>((Arc::downgrade(&mcu), room));

    application.add_http_module(signaling);

    Ok(())
}
