use crate::api::signaling::ws_modules::media::RoomControl;
use crate::modules::ApplicationBuilder;
use anyhow::Result;
use redis::aio::MultiplexedConnection;
use redis::{FromRedisValue, RedisError, RedisResult, RedisWrite, ToRedisArgs, Value};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::{from_utf8, FromStr};
use std::sync::Arc;
use uuid::Uuid;
use ws::{Echo, SignalingHttpModule};

mod mcu;
mod media;
mod storage;
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

impl ToRedisArgs for ParticipantId {
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + RedisWrite,
    {
        out.write_arg_fmt(self.0)
    }
}

pub async fn attach(
    application: &mut ApplicationBuilder,
    redis_conn: MultiplexedConnection,
    rabbit_mq_channel: lapin::Channel,
    mcu: Arc<JanusMcu>,
) -> Result<()> {
    let mut signaling = SignalingHttpModule::new(redis_conn, rabbit_mq_channel);

    signaling.add_module::<Echo>(());
    signaling.add_module::<RoomControl>(Arc::downgrade(&mcu));

    application.add_http_module(signaling);

    Ok(())
}
