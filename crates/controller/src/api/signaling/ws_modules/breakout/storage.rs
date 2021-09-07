use crate::db::rooms::RoomId;
use crate::prelude::*;
use anyhow::{Context, Result};
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::time::{Duration, SystemTime};

#[derive(Display)]
/// k3k-signaling:room={room}:breakout:rooms
#[ignore_extra_doc_attributes]
/// Typed key to a set which contains all breakout-room ids
struct BreakoutRooms {
    room: RoomId,
}

impl_to_redis_args!(BreakoutRooms);

#[derive(Display)]
/// k3k-signaling:room={room}:breakout:config
#[ignore_extra_doc_attributes]
/// Typed key to the breakout-room config for the specified room
struct BreakoutRoomConfig {
    room: RoomId,
}

impl_to_redis_args!(BreakoutRoomConfig);

/// Configuration of the current breakout rooms which lives inside redis
///
/// When the configuration is set the breakoutrooms are considered active.
/// Breakout rooms with a duration will have the redis entry expire
/// whenever the breakoutrooms expire.
#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct BreakoutConfig {
    pub rooms: u32,
    pub started: SystemTime,
    pub duration: Option<Duration>,
}

impl BreakoutConfig {
    pub fn is_valid_id(&self, id: BreakoutRoomId) -> bool {
        id.0 < self.rooms
    }
}

impl_from_redis_value_de!(BreakoutConfig);
impl_to_redis_args_se!(&BreakoutConfig);

pub async fn set_config(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    config: &BreakoutConfig,
) -> Result<()> {
    if let Some(duration) = config.duration {
        redis_conn
            .set_ex(
                BreakoutRoomConfig { room },
                config,
                duration.as_secs() as usize,
            )
            .await
            .context("Failed to set breakout-room config")
    } else {
        redis_conn
            .set(BreakoutRoomConfig { room }, config)
            .await
            .context("Failed to set breakout-room config")
    }
}

pub async fn get_config(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
) -> Result<Option<BreakoutConfig>> {
    redis_conn
        .get(BreakoutRoomConfig { room })
        .await
        .context("Failed to get breakout-room config")
}

pub async fn del_config(redis_conn: &mut ConnectionManager, room: RoomId) -> Result<()> {
    redis_conn
        .del(BreakoutRoomConfig { room })
        .await
        .context("Failed to del breakout-room config")
}
