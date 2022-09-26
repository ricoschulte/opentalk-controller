use crate::{MessageId, Scope};

use anyhow::{Context, Result};
use controller::prelude::*;
use controller_shared::ParticipantId;
use db_storage::rooms::RoomId;
use displaydoc::Display;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

/// Message type stores in redis
///
/// This needs to have a inner timestamp.
#[derive(Debug, Deserialize, Serialize)]
pub struct TimedMessage {
    pub id: MessageId,
    pub source: ParticipantId,
    pub timestamp: Timestamp,
    pub content: String,
    #[serde(flatten)]
    pub scope: Scope,
}

impl_from_redis_value_de!(TimedMessage);
impl_to_redis_args_se!(&TimedMessage);

#[derive(Display)]
/// k3k-signaling:room={room}:chat:history
#[ignore_extra_doc_attributes]
/// Key to the chat history inside a room
struct RoomChatHistory {
    room: SignalingRoomId,
}

impl_to_redis_args!(RoomChatHistory);

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn get_room_chat_history(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
) -> Result<Vec<TimedMessage>> {
    let messages = redis_conn
        .lrange(RoomChatHistory { room }, 0, -1)
        .await
        .with_context(|| format!("Failed to get chat history: room={}", room))?;

    Ok(messages)
}

#[tracing::instrument(level = "debug", skip(redis_conn, message))]
pub async fn add_message_to_room_chat_history(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
    message: &TimedMessage,
) -> Result<()> {
    redis_conn
        .lpush(RoomChatHistory { room }, message)
        .await
        .with_context(|| format!("Failed to add message to room chat history, room={}", room))?;

    Ok(())
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn delete_room_chat_history(
    redis_conn: &mut RedisConnection,
    room: SignalingRoomId,
) -> Result<()> {
    redis_conn
        .del(RoomChatHistory { room })
        .await
        .with_context(|| format!("Failed to delete room chat history, room={}", room))?;

    Ok(())
}

#[derive(Display)]
/// k3k-signaling:room={room}:chat_enabled
#[ignore_extra_doc_attributes]
/// If set to true the chat is enabled
struct ChatEnabled {
    room: RoomId,
}

impl_to_redis_args!(ChatEnabled);

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn set_chat_enabled(
    redis_conn: &mut RedisConnection,
    room: RoomId,
    enabled: bool,
) -> Result<()> {
    redis_conn
        .set(ChatEnabled { room }, enabled)
        .await
        .context("Failed to SET chat_enabled")
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn is_chat_enabled(redis_conn: &mut RedisConnection, room: RoomId) -> Result<bool> {
    redis_conn
        .get(ChatEnabled { room })
        .await
        .context("Failed to GET chat_enabled")
        .map(|result: Option<bool>| result.unwrap_or(true))
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn delete_chat_enabled(redis_conn: &mut RedisConnection, room: RoomId) -> Result<()> {
    redis_conn
        .del(ChatEnabled { room })
        .await
        .context("Failed to DEL chat_enabled")
}
