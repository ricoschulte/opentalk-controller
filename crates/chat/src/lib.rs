//! # Chat Module
//!
//! ## Functionality
//!
//! Issues timestamp and messageIds to incoming chat messages and forwards them to other participants in the room.
//! For this the rabbitmq room exchange is used.
use anyhow::Result;
use control::rabbitmq;
use controller::prelude::*;
use controller::{impl_from_redis_value_de, impl_to_redis_args_se};
use controller_shared::ParticipantId;
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::{from_utf8, FromStr};

mod storage;

#[derive(Debug, Deserialize)]
pub struct IncomingWsMessage {
    pub target: Option<ParticipantId>,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(tag = "scope", content = "target", rename_all = "snake_case")]
pub enum Scope {
    Global,
    Private(ParticipantId),
}
#[derive(Debug, Serialize, Deserialize, Clone, Copy, Eq, PartialEq)]
pub struct MessageId(uuid::Uuid);

impl MessageId {
    pub fn new() -> Self {
        MessageId(uuid::Uuid::new_v4())
    }
    #[cfg(test)]
    pub fn nil() -> Self {
        MessageId(uuid::Uuid::nil())
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl redis::FromRedisValue for MessageId {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
        match v {
            redis::Value::Data(bytes) => uuid::Uuid::from_str(from_utf8(bytes)?)
                .map(MessageId)
                .map_err(|_| {
                    redis::RedisError::from((
                        redis::ErrorKind::TypeError,
                        "invalid data for MessageId",
                    ))
                }),
            _ => redis::RedisResult::Err(redis::RedisError::from((
                redis::ErrorKind::TypeError,
                "invalid data type for MessageId",
            ))),
        }
    }
}

impl_to_redis_args!(MessageId);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Message {
    pub id: MessageId,
    pub source: ParticipantId,
    pub content: String,
    #[serde(flatten)]
    pub scope: Scope,
    // todo The timestamp is now included in the Namespaced struct. Once the frontend adopted this change, remove the timestamp from Message
    pub timestamp: Timestamp,
}

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

impl Message {
    fn with_timestamp(&self, timestamp: Timestamp) -> TimedMessage {
        TimedMessage {
            id: self.id,
            source: self.source,
            content: self.content.clone(),
            scope: self.scope,
            timestamp,
        }
    }
}

impl_from_redis_value_de!(TimedMessage);
impl_to_redis_args_se!(&TimedMessage);

pub struct Chat {
    id: ParticipantId,
    room: SignalingRoomId,
}

#[derive(Debug, Serialize)]
pub struct ChatHistory {
    room_history: Vec<TimedMessage>,
}

impl ChatHistory {
    pub async fn for_current_room(
        redis_conn: &mut ConnectionManager,
        room: SignalingRoomId,
    ) -> Result<Self> {
        let room_history = storage::get_room_chat_history(redis_conn, room).await?;
        Ok(Self { room_history })
    }
}

#[async_trait::async_trait(? Send)]
impl SignalingModule for Chat {
    const NAMESPACE: &'static str = "chat";

    type Params = ();

    type Incoming = IncomingWsMessage;
    type Outgoing = Message;
    type RabbitMqMessage = Message;

    type ExtEvent = ();

    type FrontendData = ChatHistory;
    type PeerFrontendData = ();

    async fn init(
        ctx: InitContext<'_, Self>,
        _params: &Self::Params,
        _protocol: &'static str,
    ) -> Result<Option<Self>> {
        let id = ctx.participant_id();
        let room = ctx.room_id();
        Ok(Some(Self { id, room }))
    }

    async fn on_event(
        &mut self,
        mut ctx: ModuleContext<'_, Self>,
        event: Event<'_, Self>,
    ) -> Result<()> {
        match event {
            Event::Joined {
                control_data: _,
                frontend_data,
                participants: _,
            } => {
                *frontend_data =
                    Some(ChatHistory::for_current_room(ctx.redis_conn(), self.room).await?);
            }
            Event::WsMessage(mut msg) => {
                // Discard empty messages
                if msg.content.is_empty() {
                    return Ok(());
                }

                // Limit message size to 1024 bytes at most
                if msg.content.len() > 1024 {
                    let mut last_idx = 0;

                    for (i, _) in msg.content.char_indices() {
                        if i > 1024 {
                            break;
                        }
                        last_idx = i;
                    }

                    msg.content.truncate(last_idx);
                }

                let source = self.id;

                //TODO: moderation check - mute, bad words etc., rate limit
                if let Some(target) = msg.target {
                    let out_message = Message {
                        id: MessageId::new(),
                        source,
                        content: msg.content,
                        timestamp: ctx.timestamp(),
                        scope: Scope::Private(target),
                    };

                    ctx.rabbitmq_publish(
                        rabbitmq::current_room_exchange_name(self.room),
                        rabbitmq::room_participant_routing_key(target),
                        out_message.clone(),
                    );
                    ctx.ws_send(out_message);
                } else {
                    let timestamp = ctx.timestamp();
                    let out_message = Message {
                        id: MessageId::new(),
                        source,
                        content: msg.content,
                        timestamp,
                        scope: Scope::Global,
                    };

                    // add message to room history
                    storage::add_message_to_room_chat_history(
                        ctx.redis_conn(),
                        self.room,
                        &out_message.with_timestamp(timestamp),
                    )
                    .await?;

                    ctx.rabbitmq_publish(
                        rabbitmq::current_room_exchange_name(self.room),
                        rabbitmq::room_all_routing_key().into(),
                        out_message,
                    );
                }
            }
            Event::RabbitMq(msg) => {
                ctx.ws_send(msg);
            }
            Event::Leaving
            | Event::RaiseHand
            | Event::LowerHand
            | Event::ParticipantJoined(_, _)
            | Event::ParticipantLeft(_)
            | Event::ParticipantUpdated(_, _)
            | Event::Ext(_) => {}
        }

        Ok(())
    }

    async fn on_destroy(self, mut ctx: DestroyContext<'_>) {
        if ctx.destroy_room() {
            if let Err(e) = storage::delete_room_chat_history(ctx.redis_conn(), self.room).await {
                log::error!("Failed to remove room chat history on room destroy, {}", e);
            }
        }
    }
}

pub fn register(controller: &mut controller::Controller) {
    controller.signaling.add_module::<Chat>(());
}

#[cfg(test)]
mod test {
    use super::*;
    use controller::prelude::chrono::DateTime;
    use std::str::FromStr;

    #[test]
    fn user_private_message() {
        let json = r#"
        {
            "target": "00000000-0000-0000-0000-000000000000",
            "content": "Hello Bob!"
        }
        "#;

        let msg: IncomingWsMessage = serde_json::from_str(json).unwrap();

        assert_eq!(msg.target, Some(ParticipantId::nil()));
        assert_eq!(msg.content, "Hello Bob!");
    }

    #[test]
    fn user_room_message() {
        let json = r#"
        {
            "content": "Hello all!"
        }
        "#;

        let msg: IncomingWsMessage = serde_json::from_str(json).unwrap();

        assert_eq!(msg.target, None);
        assert_eq!(msg.content, "Hello all!");
    }

    #[test]
    fn server_message() {
        let expected = r#"{"id":"00000000-0000-0000-0000-000000000000","source":"00000000-0000-0000-0000-000000000000","timestamp":"2021-06-24T14:00:11.873753715Z","content":"Hello All!","scope":"global"}"#;
        let produced = serde_json::to_string(&TimedMessage {
            id: MessageId::nil(),
            source: ParticipantId::nil(),
            timestamp: DateTime::from_str("2021-06-24T14:00:11.873753715Z")
                .unwrap()
                .into(),
            content: "Hello All!".to_string(),
            scope: Scope::Global,
        })
        .unwrap();

        assert_eq!(expected, produced);
    }

    #[test]
    fn global_serialize() {
        let produced = serde_json::to_string(&Message {
            id: MessageId::nil(),
            source: ParticipantId::nil(),
            timestamp: DateTime::from_str("2021-06-24T14:00:11.873753715Z")
                .unwrap()
                .into(),
            content: "Hello All!".to_string(),
            scope: Scope::Global,
        })
        .unwrap();
        let expected = r#"{"id":"00000000-0000-0000-0000-000000000000","source":"00000000-0000-0000-0000-000000000000","content":"Hello All!","scope":"global","timestamp":"2021-06-24T14:00:11.873753715Z"}"#;

        assert_eq!(expected, produced);
    }

    #[test]
    fn private_serialize() {
        let produced = serde_json::to_string(&Message {
            id: MessageId::nil(),
            source: ParticipantId::nil(),
            timestamp: DateTime::from_str("2021-06-24T14:00:11.873753715Z")
                .unwrap()
                .into(),
            content: "Hello All!".to_string(),
            scope: Scope::Private(ParticipantId::new_test(1)),
        })
        .unwrap();
        let expected = r#"{"id":"00000000-0000-0000-0000-000000000000","source":"00000000-0000-0000-0000-000000000000","content":"Hello All!","scope":"private","target":"00000000-0000-0000-0000-000000000001","timestamp":"2021-06-24T14:00:11.873753715Z"}"#;
        assert_eq!(expected, produced);
    }
}
