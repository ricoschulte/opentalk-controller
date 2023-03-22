// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use redis_args::{FromRedisValue, ToRedisArgs};
use serde::{Deserialize, Serialize};
use std::fmt;
use types::core::{BreakoutRoomId, RoomId};

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
    pub use super::{Role, SignalingRoomId};
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
