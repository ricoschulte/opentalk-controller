// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use super::BreakoutRoom;
use crate::prelude::*;
use crate::redis_wrapper::RedisConnection;
use anyhow::{Context, Result};
use db_storage::rooms::RoomId;
use redis::AsyncCommands;
use redis_args::{FromRedisValue, ToRedisArgs};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::time::{Duration, SystemTime};

/// Typed key to a set which contains all breakout-room ids
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:breakout:rooms")]
struct BreakoutRooms {
    room: RoomId,
}

/// Typed key to the breakout-room config for the specified room
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:breakout:config")]
struct BreakoutRoomConfig {
    room: RoomId,
}

/// Configuration of the current breakout rooms which lives inside redis
///
/// When the configuration is set the breakoutrooms are considered active.
/// Breakout rooms with a duration will have the redis entry expire
/// whenever the breakoutrooms expire.
#[derive(Debug, Serialize, Deserialize, Clone, ToRedisArgs, FromRedisValue)]
#[to_redis_args(serde)]
#[from_redis_value(serde)]
pub struct BreakoutConfig {
    pub rooms: Vec<BreakoutRoom>,
    pub started: SystemTime,
    pub duration: Option<Duration>,
}

impl BreakoutConfig {
    pub fn is_valid_id(&self, id: BreakoutRoomId) -> bool {
        self.rooms.iter().any(|room| room.id == id)
    }
}

pub async fn set_config(
    redis_conn: &mut RedisConnection,
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
    redis_conn: &mut RedisConnection,
    room: RoomId,
) -> Result<Option<BreakoutConfig>> {
    redis_conn
        .get(BreakoutRoomConfig { room })
        .await
        .context("Failed to get breakout-room config")
}

pub async fn del_config(redis_conn: &mut RedisConnection, room: RoomId) -> Result<bool> {
    redis_conn
        .del(BreakoutRoomConfig { room })
        .await
        .context("Failed to del breakout-room config")
}
