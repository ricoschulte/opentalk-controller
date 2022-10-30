// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! Shared types and trait definitions for the k3k controller.
//! One purpose is to optimize compile time during development.
use redis::{FromRedisValue, RedisError, RedisResult, Value};
use redis_args::ToRedisArgs;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::{from_utf8, FromStr};
use uuid::Uuid;

pub mod settings;

/// Unique id of a participant inside a single room
///
/// Generated as soon as the user connects to the websocket and authenticated himself,
/// it is used to store all participant related data and relations.
#[derive(
    Debug, Serialize, Deserialize, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, ToRedisArgs,
)]
#[to_redis_args(fmt)]
pub struct ParticipantId(Uuid);

impl From<Uuid> for ParticipantId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl ParticipantId {
    /// Create a ZERO participant id for testing purposes
    pub const fn nil() -> Self {
        Self(Uuid::nil())
    }

    /// Create a participant id from a number for testing purposes
    pub const fn new_test(id: u128) -> Self {
        Self(Uuid::from_u128(id))
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

impl FromRedisValue for ParticipantId {
    fn from_redis_value(v: &Value) -> RedisResult<Self> {
        match v {
            Value::Data(bytes) => Uuid::from_str(from_utf8(bytes)?)
                .map(ParticipantId)
                .map_err(|_| {
                    RedisError::from((
                        redis::ErrorKind::TypeError,
                        "invalid data for ParticipantId",
                    ))
                }),
            _ => RedisResult::Err(RedisError::from((
                redis::ErrorKind::TypeError,
                "invalid data type for ParticipantId",
            ))),
        }
    }
}
