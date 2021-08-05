use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use control::rabbitmq;
use controller::db::groups::Group;
use controller::prelude::*;
use r3dlock::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use storage::StoredMessage;
use uuid::Uuid;

mod storage;

/// Message received from websocket
#[derive(Debug, Deserialize)]
pub struct IncomingWsMessage {
    group: String,
    content: String,
}

/// Message sent via websocket and rabbitmq
#[derive(Debug, Deserialize, Serialize)]
pub struct Message {
    source: ParticipantId,
    group: String,
    timestamp: DateTime<Utc>,
    content: String,
}

fn group_routing_key(group: &str) -> String {
    format!("group.{}", group)
}

pub struct Chat {
    id: ParticipantId,
    room: Uuid,

    groups: HashSet<Group>,
}

impl Chat {
    fn is_in_group(&self, id: &str) -> bool {
        self.groups.contains(id)
    }
}

#[async_trait::async_trait(?Send)]
impl SignalingModule for Chat {
    const NAMESPACE: &'static str = "ee-chat";
    type Params = ();
    type Incoming = IncomingWsMessage;
    type Outgoing = Message;
    type RabbitMqMessage = Message;
    type ExtEvent = ();
    type FrontendData = HashMap<String, Vec<StoredMessage>>;
    type PeerFrontendData = ();

    async fn init(
        mut ctx: InitContext<'_, Self>,
        _params: &Self::Params,
        _protocol: &'static str,
    ) -> Result<Self> {
        let user_id = ctx.user().id;
        let db = ctx.db().clone();

        let groups = actix_web::web::block(move || db.get_groups_for_user(user_id))
            .await
            .context("web::block failed")?
            .context("Failed to retrieve groups for user")?;

        for group in &groups {
            log::debug!("Group: {}", group.id);

            ctx.add_rabbitmq_binding(
                group_routing_key(&group.id),
                rabbitmq::room_exchange_name(ctx.room().uuid),
                Default::default(),
            );
        }

        Ok(Self {
            id: ctx.participant_id(),
            room: ctx.room().uuid,
            groups,
        })
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
                let mut group_messages = HashMap::new();

                for group in &self.groups {
                    storage::add_participant_to_set(
                        ctx.redis_conn(),
                        self.room,
                        &group.id,
                        self.id,
                    )
                    .await?;

                    let history =
                        storage::get_group_chat_history(ctx.redis_conn(), self.room, &group.id)
                            .await?;

                    group_messages.insert(group.id.clone(), history);
                }

                *frontend_data = Some(group_messages);
            }
            Event::Leaving => {}
            Event::ParticipantJoined(_, _) => {}
            Event::ParticipantLeft(_) => {}
            Event::ParticipantUpdated(_, _) => {}
            Event::WsMessage(msg) => {
                if self.is_in_group(&msg.group) {
                    let stored_msg = StoredMessage {
                        source: self.id,
                        timestamp: Utc::now(),
                        content: msg.content,
                    };

                    storage::add_message_to_group_chat_history(
                        ctx.redis_conn(),
                        self.room,
                        &msg.group,
                        &stored_msg,
                    )
                    .await?;

                    ctx.rabbitmq_publish(
                        rabbitmq::room_exchange_name(self.room),
                        group_routing_key(&msg.group),
                        Message {
                            source: self.id,
                            group: msg.group,
                            timestamp: Utc::now(),
                            content: stored_msg.content,
                        },
                    )
                }
            }
            Event::RabbitMq(msg) => {
                if self.is_in_group(&msg.group) {
                    ctx.ws_send(msg);
                }
            }
            Event::Ext(_) => {}
        }

        Ok(())
    }

    async fn on_destroy(self, mut ctx: DestroyContext<'_>) {
        for group in self.groups {
            let mut mutex = Mutex::new(storage::RoomGroupParticipantsLock {
                room: self.room,
                group: &group.id,
            });

            let guard = match mutex.lock(ctx.redis_conn()).await {
                Ok(guard) => guard,
                Err(e) => {
                    log::error!(
                        "Failed to acquire lock to cleanup group {}, {}",
                        group.id,
                        e
                    );
                    continue;
                }
            };

            let remove_history = match storage::remove_participant_from_set(
                &guard,
                ctx.redis_conn(),
                self.room,
                &group.id,
                self.id,
            )
            .await
            {
                Ok(n) => n == 0,
                Err(e) => {
                    log::error!("Failed to remove participant from group set, {}", e);
                    false
                }
            };

            if remove_history {
                if let Err(e) =
                    storage::delete_group_chat_history(ctx.redis_conn(), self.room, &group.id).await
                {
                    log::error!("Failed to remove room group chat histroy, {}", e);
                }
            }

            if let Err(e) = guard.unlock(ctx.redis_conn()).await {
                log::error!("Failed to unlock r3dlock, {}", e);
            }
        }
    }
}

pub fn register(controller: &mut controller::Controller) {
    controller.signaling.add_module::<Chat>(());
}
