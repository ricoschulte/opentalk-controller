//! # Chat Module
//!
//! ## Functionality
//!
//! Issues timestamp and messageIds to incoming chat messages and forwards them to other participants in the room.
//! For this the rabbitmq room exchange is used.
use anyhow::Result;
use control::rabbitmq;
use controller::prelude::*;
use controller_shared::ParticipantId;
use outgoing::{ChatDisabled, ChatEnabled, MessageSent};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::{from_utf8, FromStr};
use storage::TimedMessage;

pub mod incoming;
pub mod outgoing;
mod storage;

pub use storage::is_chat_enabled;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
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

pub struct Chat {
    id: ParticipantId,
    room: SignalingRoomId,
}

#[derive(Debug, Serialize)]
pub struct ChatState {
    room_history: Vec<TimedMessage>,
    enabled: bool,
}

impl ChatState {
    pub async fn for_current_room(
        redis_conn: &mut RedisConnection,
        room: SignalingRoomId,
    ) -> Result<Self> {
        let room_history = storage::get_room_chat_history(redis_conn, room).await?;
        let chat_enabled = storage::is_chat_enabled(redis_conn, room.room_id()).await?;

        Ok(Self {
            room_history,
            enabled: chat_enabled,
        })
    }
}

#[async_trait::async_trait(? Send)]
impl SignalingModule for Chat {
    const NAMESPACE: &'static str = "chat";

    type Params = ();

    type Incoming = incoming::Message;
    type Outgoing = outgoing::Message;
    type RabbitMqMessage = outgoing::Message;

    type ExtEvent = ();

    type FrontendData = ChatState;
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
                let module_frontend_data =
                    ChatState::for_current_room(ctx.redis_conn(), self.room).await?;

                *frontend_data = Some(module_frontend_data);
            }
            Event::WsMessage(incoming::Message::EnableChat) => {
                if ctx.role() != Role::Moderator {
                    ctx.ws_send(outgoing::Message::Error(
                        outgoing::Error::InsufficientPermissions,
                    ));
                    return Ok(());
                }

                storage::set_chat_enabled(ctx.redis_conn(), self.room.room_id(), true).await?;

                ctx.rabbitmq_publish(
                    rabbitmq::current_room_exchange_name(self.room),
                    rabbitmq::room_all_routing_key().into(),
                    outgoing::Message::ChatEnabled(ChatEnabled { issued_by: self.id }),
                );
            }
            Event::WsMessage(incoming::Message::DisableChat) => {
                if ctx.role() != Role::Moderator {
                    ctx.ws_send(outgoing::Message::Error(
                        outgoing::Error::InsufficientPermissions,
                    ));
                    return Ok(());
                }

                storage::set_chat_enabled(ctx.redis_conn(), self.room.room_id(), false).await?;

                ctx.rabbitmq_publish(
                    rabbitmq::current_room_exchange_name(self.room),
                    rabbitmq::room_all_routing_key().into(),
                    outgoing::Message::ChatDisabled(ChatDisabled { issued_by: self.id }),
                );
            }
            Event::WsMessage(incoming::Message::SendMessage {
                target,
                mut content,
            }) => {
                // Discard empty messages
                if content.is_empty() {
                    return Ok(());
                }

                let chat_enabled =
                    storage::is_chat_enabled(ctx.redis_conn(), self.room.room_id()).await?;

                if !chat_enabled {
                    ctx.ws_send(outgoing::Message::Error(outgoing::Error::ChatDisabled));
                    return Ok(());
                }

                // Limit message size
                let max_message_size = 4096;
                if content.len() > max_message_size {
                    let mut last_idx = 0;

                    for (i, _) in content.char_indices() {
                        if i > max_message_size {
                            break;
                        }
                        last_idx = i;
                    }

                    content.truncate(last_idx);
                }

                let source = self.id;

                //TODO: moderation check - mute, bad words etc., rate limit
                if let Some(target) = target {
                    let out_message = outgoing::Message::MessageSent(MessageSent {
                        id: MessageId::new(),
                        source,
                        content,
                        timestamp: ctx.timestamp(),
                        scope: Scope::Private(target),
                    });

                    ctx.rabbitmq_publish(
                        rabbitmq::current_room_exchange_name(self.room),
                        rabbitmq::room_participant_routing_key(target),
                        out_message.clone(),
                    );
                    ctx.ws_send(out_message);
                } else {
                    let out_message_contents = MessageSent {
                        id: MessageId::new(),
                        source,
                        content,
                        timestamp: ctx.timestamp(),
                        scope: Scope::Global,
                    };

                    let timed_message = TimedMessage {
                        id: out_message_contents.id,
                        source: out_message_contents.source,
                        content: out_message_contents.content.clone(),
                        scope: out_message_contents.scope,
                        timestamp: out_message_contents.timestamp,
                    };

                    storage::add_message_to_room_chat_history(
                        ctx.redis_conn(),
                        self.room,
                        &timed_message,
                    )
                    .await?;

                    let out_message = outgoing::Message::MessageSent(out_message_contents);

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
            if let Err(e) =
                storage::delete_chat_enabled(ctx.redis_conn(), self.room.room_id()).await
            {
                log::error!("Failed to clean up chat enabled flag {}", e);
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
}
