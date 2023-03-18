// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use std::ops::Deref;

use chrono::{DateTime, TimeZone as _, Utc};

use crate::imports::*;

/// A UTC DateTime wrapper that implements ToRedisArgs and FromRedisValue.
///
/// The values are stores as unix timestamps in redis.
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Timestamp(DateTime<Utc>);

impl Timestamp {
    /// Create a timestamp with the date of the unix epoch start
    /// (1970-01-01 00:00:00)
    pub fn unix_epoch() -> Self {
        Self(chrono::DateTime::from(std::time::UNIX_EPOCH))
    }

    /// Create a timestamp with the current system time
    pub fn now() -> Timestamp {
        Timestamp(Utc::now())
    }
}

impl From<DateTime<Utc>> for Timestamp {
    fn from(value: DateTime<Utc>) -> Self {
        Timestamp(value)
    }
}

#[cfg(feature = "redis")]
impl ToRedisArgs for Timestamp {
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

#[cfg(feature = "redis")]
impl FromRedisValue for Timestamp {
    fn from_redis_value(v: &redis::Value) -> RedisResult<Timestamp> {
        let timestamp = Utc
            .timestamp_opt(i64::from_redis_value(v)?, 0)
            .latest()
            .unwrap();
        Ok(Timestamp(timestamp))
    }
}

impl Deref for Timestamp {
    type Target = DateTime<Utc>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
