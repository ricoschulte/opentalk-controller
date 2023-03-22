// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use uuid::Uuid;

use crate::imports::*;

/// The id of a breakout room
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "redis", derive(ToRedisArgs), to_redis_args(fmt = "{}"))]
pub struct BreakoutRoomId(Uuid);

impl BreakoutRoomId {
    /// Create a ZERO breakout room id, e.g. for testing purposes
    pub const fn nil() -> Self {
        Self(Uuid::nil())
    }

    /// Create a breakout id from a number, e.g. for testing purposes
    pub const fn from_u128(id: u128) -> Self {
        Self(Uuid::from_u128(id))
    }

    /// Generate a new random breakout room id
    #[cfg(feature = "rand")]
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl From<Uuid> for BreakoutRoomId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for BreakoutRoomId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
