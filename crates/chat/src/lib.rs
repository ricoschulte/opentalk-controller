use anyhow::Result;
use chrono::{DateTime, Utc};
use control::rabbitmq;
use controller::db::rooms::RoomId;
use controller::prelude::*;
use controller::{impl_from_redis_value_de, impl_to_redis_args_se};
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};

mod storage;

#[derive(Debug, Deserialize)]
pub struct IncomingWsMessage {
    pub target: Option<ParticipantId>,
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
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

impl_from_redis_value_de!(Message);
impl_to_redis_args_se!(&Message);

pub struct Chat {
    id: ParticipantId,
    room: RoomId,
}

#[derive(Debug, Serialize)]
pub struct ChatHistory {
    room_history: Vec<Message>,
}

impl ChatHistory {
    pub async fn for_current_room(
        redis_conn: &mut ConnectionManager,
        room: RoomId,
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
    ) -> Result<Self> {
        let id = ctx.participant_id();
        let room = ctx.room().uuid;
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
        let expected = r#"{"source":"00000000-0000-0000-0000-000000000000","timestamp":"2021-06-24T14:00:11.873753715Z","content":"Hello All!","scope":"global"}"#;
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
