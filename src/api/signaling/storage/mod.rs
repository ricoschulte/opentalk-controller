use crate::api::signaling::ParticipantId;
use anyhow::{Context, Result};
use key::RedisKey;
use redis::{AsyncCommands, FromRedisValue, ToRedisArgs};
use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::Debug;
use uuid::Uuid;

mod key;

pub struct Storage {
    room: Uuid,

    // Room related redis change events
    redis_conn: redis::aio::MultiplexedConnection,
}

impl Storage {
    pub fn new(redis_conn: redis::aio::MultiplexedConnection, room: Uuid) -> Self {
        Self { room, redis_conn }
    }

    pub async fn get_participants(&mut self) -> Result<HashSet<ParticipantId>> {
        let participants: HashSet<ParticipantId> = self
            .redis_conn
            .smembers(RedisKey::RoomParticipants(self.room))
            .await
            .context("Failed to get participants")?;

        Ok(participants)
    }

    pub async fn add_participant_to_set(&mut self, participant: ParticipantId) -> Result<()> {
        self.redis_conn
            .sadd(RedisKey::RoomParticipants(self.room), participant)
            .await
            .context("Failed to add own participant id to set")?;

        Ok(())
    }

    pub async fn remove_participant_from_set(&mut self, participant: ParticipantId) -> Result<()> {
        self.redis_conn
            .srem(RedisKey::RoomParticipants(self.room), participant)
            .await
            .context("Failed to remove participant from participants-set")
    }

    pub async fn remove_all_attributes(
        &mut self,
        namespace: &str,
        participant: ParticipantId,
    ) -> Result<()> {
        self.redis_conn
            .del(RedisKey::RoomParticipant(
                self.room,
                participant,
                Cow::Borrowed(namespace),
            ))
            .await
            .context("Failed to remove participant field")
    }

    pub async fn set_attribute<K, V>(
        &mut self,
        namespace: &str,
        participant: ParticipantId,
        key: K,
        value: V,
    ) -> Result<()>
    where
        K: ToRedisArgs + Send + Sync,
        V: ToRedisArgs + Send + Sync,
    {
        self.redis_conn
            .hset(
                RedisKey::RoomParticipant(self.room, participant, Cow::Borrowed(namespace)),
                key,
                value,
            )
            .await
            .context("Failed to set attribute")?;

        Ok(())
    }

    pub async fn get_attribute<K, V>(
        &mut self,
        namespace: &str,
        participant: ParticipantId,
        key: K,
    ) -> Result<V>
    where
        K: ToRedisArgs + Send + Sync + Debug + Copy,
        V: FromRedisValue,
    {
        let value = self
            .redis_conn
            .hget(
                RedisKey::RoomParticipant(self.room, participant, Cow::Borrowed(namespace)),
                key,
            )
            .await
            .with_context(|| format!("Failed to get attribute {:?}", key))?;

        Ok(value)
    }

    pub async fn remove_attribute<K>(
        &mut self,
        namespace: &str,
        participant: ParticipantId,
        key: K,
    ) -> Result<()>
    where
        K: ToRedisArgs + Send + Sync,
    {
        self.redis_conn
            .hdel(
                RedisKey::RoomParticipant(self.room, participant, Cow::Borrowed(namespace)),
                key,
            )
            .await
            .context("Failed to remove attribute")?;

        Ok(())
    }
}
