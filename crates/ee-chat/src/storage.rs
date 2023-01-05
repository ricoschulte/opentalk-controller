use anyhow::{Context, Result};
use chat::MessageId;
use chrono::{DateTime, Utc};
use controller::prelude::*;
use controller_shared::ParticipantId;
use db_storage::groups::GroupId;
use r3dlock::{Mutex, MutexGuard};
use redis::AsyncCommands;
use redis_args::{FromRedisValue, ToRedisArgs};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A set of group members inside a room
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:group={group}:participants")]
struct RoomGroupParticipants {
    room: SignalingRoomId,
    group: GroupId,
}

// A lock for the set of group members inside a room
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:group={group}:participants.lock")]
pub struct RoomGroupParticipantsLock {
    pub room: SignalingRoomId,
    pub group: GroupId,
}

/// A lock for the set of group members inside a room
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:group={group}:chat:history")]
struct RoomGroupChatHistory {
    room: SignalingRoomId,
    group: GroupId,
}

pub async fn add_participant_to_set(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    group: GroupId,
    participant: ParticipantId,
) -> Result<()> {
    let mut mutex = Mutex::new(RoomGroupParticipantsLock { room, group });

    let guard = mutex
        .lock(redis_conn)
        .await
        .context("Failed to lock participant list")?;

    redis_conn
        .sadd(RoomGroupParticipants { room, group }, participant)
        .await
        .context("Failed to add own participant id to set")?;

    guard
        .unlock(redis_conn)
        .await
        .context("Failed to unlock participant list")?;

    Ok(())
}

pub async fn remove_participant_from_set(
    _set_guard: &MutexGuard<'_, RoomGroupParticipantsLock>,
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    group: GroupId,
    participant: ParticipantId,
) -> Result<usize> {
    redis_conn
        .srem(RoomGroupParticipants { room, group }, participant)
        .await
        .context("Failed to remove participant from participants-set")?;

    redis_conn
        .scard(RoomGroupParticipants { room, group })
        .await
        .context("Failed to get number of remaining participants inside the set")
}

/// Message stored inside redis and sent to frontend on `join_success`
#[derive(Debug, Deserialize, Serialize, ToRedisArgs, FromRedisValue)]
#[to_redis_args(serde)]
#[from_redis_value(serde)]
pub struct StoredMessage {
    pub id: MessageId,
    pub source: ParticipantId,
    pub timestamp: DateTime<Utc>,
    pub content: String,
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn get_group_chat_history(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    group: GroupId,
) -> Result<Vec<StoredMessage>> {
    redis_conn
        .lrange(RoomGroupChatHistory { room, group }, 0, -1)
        .await
        .with_context(|| format!("Failed to get chat history, {}, group={}", room, group))
}

#[tracing::instrument(level = "debug", skip(redis_conn, message))]
pub async fn add_message_to_group_chat_history(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    group: GroupId,
    message: &StoredMessage,
) -> Result<()> {
    redis_conn
        .lpush(RoomGroupChatHistory { room, group }, message)
        .await
        .with_context(|| {
            format!(
                "Failed to add message to room chat history, {}, group={}",
                room, group
            )
        })
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn delete_group_chat_history(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    group: GroupId,
) -> Result<()> {
    redis_conn
        .del(RoomGroupChatHistory { room, group })
        .await
        .with_context(|| {
            format!(
                "Failed to delete room group chat history, {}, group={}",
                room, group
            )
        })
}

/// A hash of last-seen timestamps
#[derive(ToRedisArgs)]
#[to_redis_args(fmt = "k3k-signaling:room={room}:participant={participant}:chat:last_seen:group")]
struct RoomParticipantLastSeenTimestampsGroup {
    room: SignalingRoomId,
    participant: ParticipantId,
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn set_last_seen_timestamps_group(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    participant: ParticipantId,
    timestamps: &[(String, Timestamp)],
) -> Result<()> {
    redis_conn
        .hset_multiple(
            RoomParticipantLastSeenTimestampsGroup { room, participant },
            timestamps,
        )
        .await
        .context("Failed to HSET messages last seen timestamp for group chats")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn get_last_seen_timestamps_group(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<HashMap<String, Timestamp>> {
    redis_conn
        .hgetall(RoomParticipantLastSeenTimestampsGroup { room, participant })
        .await
        .context("Failed to HGETALL messages last seen timestamp for group chats")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn delete_last_seen_timestamps_group(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<()> {
    redis_conn
        .del(RoomParticipantLastSeenTimestampsGroup { room, participant })
        .await
        .context("Failed to DEL last seen timestamp for group chats")
}
