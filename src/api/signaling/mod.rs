use redis::{FromRedisValue, RedisError, RedisResult, Value};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::{from_utf8, FromStr};
use uuid::Uuid;

/// Implement [`redis::ToRedisArgs`] for a type that implements display
macro_rules! impl_to_redis_args {
    ($ty:ty) => {
        impl ::redis::ToRedisArgs for $ty {
            fn write_redis_args<W>(&self, out: &mut W)
            where
                W: ?Sized + ::redis::RedisWrite,
            {
                out.write_arg_fmt(self)
            }
        }
    };
}

/// Implement [`redis::FromRedisValue`] for a type that implements deserialize
macro_rules! impl_from_redis_value_de {
    ($ty:ty) => {
        impl ::redis::FromRedisValue for $ty {
            fn from_redis_value(v: &::redis::Value) -> ::redis::RedisResult<$ty> {
                match *v {
                    ::redis::Value::Data(ref bytes) => {
                        ::serde_json::from_slice(bytes).map_err(|_| {
                            ::redis::RedisError::from((
                                ::redis::ErrorKind::TypeError,
                                "invalid data content",
                            ))
                        })
                    }
                    _ => ::redis::RedisResult::Err(::redis::RedisError::from((
                        ::redis::ErrorKind::TypeError,
                        "invalid data type",
                    ))),
                }
            }
        }
    };
}

/// Implement [`redis::ToRedisArgs`] for a type that implements serialize
macro_rules! impl_to_redis_args_se {
    ($ty:ty) => {
        impl ::redis::ToRedisArgs for $ty {
            fn write_redis_args<W>(&self, out: &mut W)
            where
                W: ?Sized + ::redis::RedisWrite,
            {
                let json_val = ::serde_json::to_vec(self).expect("Failed to serialize");
                out.write_arg(&json_val);
            }
        }
    };
}

mod mcu;
mod ws;
mod ws_modules;

pub mod ce {
    pub use super::ws::Echo;
    pub use super::ws_modules::chat::Chat;
    pub use super::ws_modules::media::Media;
}

pub mod ee {
    pub use super::ws_modules::ee::chat::Chat;
}

pub use mcu::McuPool;
pub use ws::SignalingHttpModule;

#[derive(Debug, Serialize, Deserialize, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct ParticipantId(Uuid);

impl ParticipantId {
    #[cfg(test)]
    fn nil() -> Self {
        Self(Uuid::nil())
    }

    fn new() -> Self {
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

impl_to_redis_args!(ParticipantId);
