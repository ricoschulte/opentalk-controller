use super::Message;
use anyhow::{Context, Result};
use controller::db::rooms::RoomId;
use controller::prelude::*;
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room}:chat:history
#[ignore_extra_doc_attributes]
/// Key to the chat history inside a room
struct RoomChatHistory {
    room: RoomId,
}

impl_to_redis_args!(RoomChatHistory);

pub async fn get_room_chat_history(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
) -> Result<Vec<Message>> {
    let messages = redis_conn
        .lrange(RoomChatHistory { room }, 0, -1)
        .await
        .with_context(|| format!("Failed to get chat history: room={}", room))?;

    Ok(messages)
}

pub async fn add_message_to_room_chat_history(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    message: &Message,
) -> Result<()> {
    redis_conn
        .lpush(RoomChatHistory { room }, message)
        .await
        .with_context(|| format!("Failed to add message to room chat history, room={}", room))?;

    Ok(())
}

pub async fn delete_room_chat_history(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
) -> Result<()> {
    redis_conn
        .del(RoomChatHistory { room })
        .await
        .with_context(|| format!("Failed to delete room chat history, room={}", room))?;

    Ok(())
}
