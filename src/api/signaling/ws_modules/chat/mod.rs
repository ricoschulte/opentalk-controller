use crate::api::signaling::storage::Storage;
use crate::api::signaling::ws::{Event, ModuleContext, SignalingModule};
use crate::api::signaling::ParticipantId;
use anyhow::Result;
use chrono::{DateTime, Utc};
use redis::{FromRedisValue, RedisError, RedisResult, RedisWrite, ToRedisArgs};
use serde::{Deserialize, Serialize};
use serde_json;

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
}

#[derive(Debug, Serialize)]
pub struct ChatHistory {
    room_history: Vec<Message>,
}

impl ChatHistory {
    pub async fn for_current_room(storage: &mut Storage) -> Result<Self> {
        let room_history: Vec<Message> = storage.get_items(NAMESPACE_HISTORY).await?;
        Ok(Self { room_history })
    }
}

const NAMESPACE_HISTORY: &str = "chat.history";

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
        ctx: ModuleContext<'_, Self>,
        _params: &Self::Params,
        _protocol: &'static str,
    ) -> Result<Self> {
        let id = ctx.participant_id();
        Ok(Self { id })
    }

    async fn on_event(
        &mut self,
        mut ctx: ModuleContext<'_, Self>,
        event: Event<Self>,
    ) -> Result<()> {
        match event {
            Event::ParticipantJoined(_, _) => {}
            Event::WsMessage(msg) => {
                let source = ctx.participant_id();
                let timestamp = Utc::now();

                //TODO: moderation check - mute, bad words etc., rate limit
                if let Some(target) = msg.target {
                    let out_message = Message {
                        source,
                        timestamp,
                        content: msg.content,
                        scope: Scope::Private,
                    };

                    ctx.rabbitmq_send(Some(target), out_message);
                } else {
                    let out_message = Message {
                        source,
                        timestamp,
                        content: msg.content,
                        scope: Scope::Global,
                    };
                    // add message to room history
                    ctx.storage()
                        .add_item(NAMESPACE_HISTORY, &out_message)
                        .await?;
                    ctx.rabbitmq_send(None, out_message);
                }
            }
            Event::RabbitMq(msg) => {
                ctx.ws_send(msg);
            }
            Event::ParticipantLeft(_) | Event::ParticipantUpdated(_, _) | Event::Ext(_) => {}
        }

        Ok(())
    }

    async fn get_frontend_data(&self, storage: &mut Storage) -> Result<Self::FrontendData> {
        ChatHistory::for_current_room(storage).await
    }

    async fn get_frontend_data_for(
        &self,
        _storage: &mut Storage,
        _participant: ParticipantId,
    ) -> Result<Self::PeerFrontendData> {
        Ok(())
    }

    async fn on_destroy(self, storage: &mut Storage) {
        // TODO: race condition possile here. Use r3dlock to ensure mutual exclusion
        let remaining_users = storage
            .get_participants()
            .await
            .expect("Can not fetch remaining room users from redis");

        if remaining_users.is_empty() {
            storage
                .remove_namespace(NAMESPACE_HISTORY)
                .await
                .expect("failed to clean room chat history on close.");
            log::debug!("Clean up room after last user ({:#?})", self.id);
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
