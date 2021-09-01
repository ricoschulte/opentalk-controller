use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use controller::db::rooms::RoomId;
use controller::prelude::*;
use displaydoc::Display;
use r3dlock::{Mutex, MutexGuard};
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

#[derive(Display)]
/// k3k-signaling:room={room}:group={group}:participants
#[ignore_extra_doc_attributes]
/// A set of group members inside a room
struct RoomGroupParticipants<'s> {
    room: RoomId,
    group: &'s str,
}

#[derive(Display)]
/// k3k-signaling:room={room}:group={group}:participants.lock
#[ignore_extra_doc_attributes]
/// A lock for the set of group members inside a room
pub struct RoomGroupParticipantsLock<'s> {
    pub room: RoomId,
    pub group: &'s str,
}

#[derive(Display)]
/// k3k-signaling:room={room}:group={group}:chat:history
#[ignore_extra_doc_attributes]
/// A lock for the set of group members inside a room
struct RoomGroupChatHistory<'s> {
    room: RoomId,
    group: &'s str,
}

impl_to_redis_args!(RoomGroupParticipants<'_>);
impl_to_redis_args!(RoomGroupParticipantsLock<'_>);
impl_to_redis_args!(RoomGroupChatHistory<'_>);

pub async fn add_participant_to_set(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    group: &str,
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
    _set_guard: &MutexGuard<'_, RoomGroupParticipantsLock<'_>>,
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    group: &str,
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
#[derive(Debug, Deserialize, Serialize)]
pub struct StoredMessage {
    pub source: ParticipantId,
    pub timestamp: DateTime<Utc>,
    pub content: String,
}

impl_from_redis_value_de!(StoredMessage);
impl_to_redis_args_se!(&StoredMessage);

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn get_group_chat_history(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    group: &str,
) -> Result<Vec<StoredMessage>> {
    redis_conn
        .lrange(RoomGroupChatHistory { room, group }, 0, -1)
        .await
        .with_context(|| format!("Failed to get chat history, room={}, group={}", room, group))
}

#[tracing::instrument(level = "debug", skip(redis_conn, message))]
pub async fn add_message_to_group_chat_history(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    group: &str,
    message: &StoredMessage,
) -> Result<()> {
    redis_conn
        .lpush(RoomGroupChatHistory { room, group }, message)
        .await
        .with_context(|| {
            format!(
                "Failed to add message to room chat history, room={}, group={}",
                room, group
            )
        })
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn delete_group_chat_history(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    group: &str,
) -> Result<()> {
    redis_conn
        .del(RoomGroupChatHistory { room, group })
        .await
        .with_context(|| {
            format!(
                "Failed to delete room group chat history, room={}, group={}",
                room, group
            )
        })
}
