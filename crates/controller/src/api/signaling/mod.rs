// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::api::signaling::ws_modules::breakout::BreakoutRoomId;
use chrono::TimeZone;
use db_storage::rooms::RoomId;
use redis_args::{FromRedisValue, ToRedisArgs};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;

pub(crate) mod metrics;
pub(crate) mod resumption;
pub(crate) mod ticket;

mod ws;
mod ws_modules;

pub(crate) use ws::ws_service;

pub mod prelude {
    pub use super::ws::module_tester::*;
    pub use super::ws::{
        DestroyContext, Event, InitContext, ModuleContext, SignalingModule, SignalingModules,
        SignalingProtocols,
    };
    pub use super::ws_modules::{breakout, control, moderation, recording};
    pub use super::{Role, SignalingRoomId, Timestamp};
    pub use breakout::BreakoutRoomId;
}

/// Role of the participant inside a room
#[derive(
    Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, ToRedisArgs, FromRedisValue,
)]
#[serde(rename_all = "lowercase")]
#[to_redis_args(serde)]
#[from_redis_value(serde)]
pub enum Role {
    Guest,
    User,
    Moderator,
}

/// The complete room id
///
/// It consist of the room-id inside the database and an optional
/// breakout-room-id which is generated when the breakout rooms are created
#[derive(Debug, Copy, Clone)]
pub struct SignalingRoomId(RoomId, Option<BreakoutRoomId>);

impl SignalingRoomId {
    pub const fn new_test(room: RoomId) -> Self {
        Self(room, None)
    }

    pub const fn room_id(&self) -> RoomId {
        self.0
    }

    pub const fn breakout_room_id(&self) -> Option<BreakoutRoomId> {
        self.1
    }
}

impl fmt::Display for SignalingRoomId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(breakout) = self.1 {
            write!(f, "{}:{}", self.0, breakout)
        } else {
            self.0.fmt(f)
        }
    }
}

/// A UTC DateTime wrapper that implements ToRedisArgs and FromRedisValue.
///
/// The values are stores as unix timestamps in redis.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Timestamp(chrono::DateTime<chrono::Utc>);

impl Timestamp {
    fn unix_epoch() -> Self {
        Self(chrono::DateTime::from(std::time::UNIX_EPOCH))
    }

    fn now() -> Timestamp {
        Timestamp(chrono::Utc::now())
    }
}

impl From<chrono::DateTime<chrono::Utc>> for Timestamp {
    fn from(value: chrono::DateTime<chrono::Utc>) -> Self {
        Timestamp(value)
    }
}

impl redis::ToRedisArgs for Timestamp {
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + redis::RedisWrite,
    {
        self.0.timestamp().write_redis_args(out)
    }

    fn describe_numeric_behavior(&self) -> redis::NumericBehavior {
        redis::NumericBehavior::NumberIsInteger
    }
}

impl redis::FromRedisValue for Timestamp {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Timestamp> {
        let timestamp = chrono::Utc
            .timestamp_opt(i64::from_redis_value(v)?, 0)
            .latest()
            .unwrap();
        Ok(Timestamp(timestamp))
    }
}

impl Deref for Timestamp {
    type Target = chrono::DateTime<chrono::Utc>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
