use super::TimedMessage;
use anyhow::{Context, Result};
use controller::prelude::*;
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

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
    redis_conn: &mut ConnectionManager,
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
    redis_conn: &mut ConnectionManager,
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
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
) -> Result<()> {
    redis_conn
        .del(RoomChatHistory { room })
        .await
        .with_context(|| format!("Failed to delete room chat history, room={}", room))?;

    Ok(())
}
