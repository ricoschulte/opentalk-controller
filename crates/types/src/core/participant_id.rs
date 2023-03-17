// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use std::str::{from_utf8, FromStr};

use redis::RedisError;
use uuid::Uuid;

use crate::imports::*;

/// Unique id of a participant inside a single room
///
/// Generated as soon as the user connects to the websocket and authenticated himself,
/// it is used to store all participant related data and relations.
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "redis", derive(ToRedisArgs), to_redis_args(fmt))]
pub struct ParticipantId(Uuid);

impl From<Uuid> for ParticipantId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl ParticipantId {
    /// Create a ZERO participant id, e.g. for testing purposes
    pub const fn nil() -> Self {
        Self(Uuid::nil())
    }

    /// Create a participant id from a number for testing purposes
    pub const fn from_u128(id: u128) -> Self {
        Self(Uuid::from_u128(id))
    }

    /// Generate a new random participant id
    #[cfg(feature = "rand")]
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for ParticipantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(feature = "redis")]
impl FromRedisValue for ParticipantId {
    fn from_redis_value(v: &redis::Value) -> RedisResult<Self> {
        match v {
            redis::Value::Data(bytes) => Uuid::from_str(from_utf8(bytes)?)
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
