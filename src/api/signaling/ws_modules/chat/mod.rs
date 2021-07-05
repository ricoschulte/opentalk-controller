use super::control::rabbitmq;
use crate::api::signaling::ws::{
    DestroyContext, Event, InitContext, ModuleContext, SignalingModule,
};
use crate::api::signaling::ParticipantId;
use anyhow::Result;
use chrono::{DateTime, Utc};
use redis::aio::ConnectionManager;
use redis::{FromRedisValue, RedisError, RedisResult, RedisWrite, ToRedisArgs};
use serde::{Deserialize, Serialize};
use serde_json;
use uuid::Uuid;

mod storage;

#[derive(Debug, Deserialize)]
pub struct IncomingWsMessage {
    pub target: Option<ParticipantId>,
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum Scope {
    Global,
    Private,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Message {
    pub source: ParticipantId,
    pub timestamp: DateTime<Utc>,
    pub content: String,
    pub scope: Scope,
}

impl FromRedisValue for Message {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Message> {
        match *v {
            redis::Value::Data(ref bytes) => serde_json::from_slice(bytes).map_err(|_| {
                RedisError::from((redis::ErrorKind::TypeError, "invalid data for Message"))
            }),
            _ => RedisResult::Err(RedisError::from((
                redis::ErrorKind::TypeError,
                "invalid data type for Message",
            ))),
        }
    }
}

impl ToRedisArgs for &Message {
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + RedisWrite,
    {
        let json_val = serde_json::to_vec(self).expect("Can not serialize message");
        out.write_arg(&json_val);
    }
}

pub struct Chat {
    id: ParticipantId,
    room: Uuid,
}

#[derive(Debug, Serialize)]
pub struct ChatHistory {
    room_history: Vec<Message>,
}

impl ChatHistory {
    pub async fn for_current_room(redis_conn: &mut ConnectionManager, room: Uuid) -> Result<Self> {
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
    ) -> Result<Self> {
        let id = ctx.participant_id();
        let room = ctx.room_id();
        Ok(Self { id, room })
    }

    async fn on_event(
        &mut self,
        mut ctx: ModuleContext<'_, Self>,
        event: Event<'_, Self>,
    ) -> Result<()> {
        match event {
            Event::Joined {
                frontend_data,
                participants: _,
            } => {
                *frontend_data =
                    Some(ChatHistory::for_current_room(ctx.redis_conn(), self.room).await?);
            }
            Event::ParticipantJoined(_, _) => {}
            Event::WsMessage(msg) => {
                let source = self.id;
                let timestamp = Utc::now();

                //TODO: moderation check - mute, bad words etc., rate limit
                if let Some(target) = msg.target {
                    let out_message = Message {
                        source,
                        timestamp,
                        content: msg.content,
                        scope: Scope::Private,
                    };

                    ctx.rabbitmq_publish(
                        rabbitmq::room_exchange_name(self.room),
                        rabbitmq::room_participant_routing_key(target),
                        out_message,
                    );
                } else {
                    let out_message = Message {
                        source,
                        timestamp,
                        content: msg.content,
                        scope: Scope::Global,
                    };

                    // add message to room history
                    storage::add_message_to_room_chat_history(
                        ctx.redis_conn(),
                        self.room,
                        &out_message,
                    )
                    .await?;

                    ctx.rabbitmq_publish(
                        rabbitmq::room_exchange_name(self.room),
                        rabbitmq::room_participant_routing_key(self.id),
                        out_message,
                    );
                }
            }
            Event::RabbitMq(msg) => {
                ctx.ws_send(msg);
            }
            Event::ParticipantLeft(_) | Event::ParticipantUpdated(_, _) | Event::Ext(_) => {}
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

#[cfg(test)]
mod test {
    use super::*;
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
        let expected = r#"{"source":"00000000-0000-0000-0000-000000000000","timestamp":"2021-06-24T14:00:11.873753715Z","content":"Hello All!","scope":"Global"}"#;
        let produced = serde_json::to_string(&Message {
            source: ParticipantId::nil(),
            timestamp: DateTime::from_str("2021-06-24T14:00:11.873753715Z").unwrap(),
            content: "Hello All!".to_string(),
            scope: Scope::Global,
        })
        .unwrap();

        assert_eq!(expected, produced);
    }
}