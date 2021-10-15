//! # EE Chat Module
//!
//! ## Functionality
//!
//! Issues timestamp and messageIds to incoming chat messages and forwards them to other participants in a group.
//! For this the rabbitmq target group exchange is used.
use anyhow::{Context, Result};
use chat::MessageId;
use chrono::{DateTime, Utc};
use control::rabbitmq;
use controller::db::groups::Group;
use controller::prelude::*;
use r3dlock::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use storage::StoredMessage;

mod storage;

/// Message received from websocket
#[derive(Debug, Deserialize)]
pub struct IncomingWsMessage {
    group: String,
    content: String,
}

/// Message sent via websocket and rabbitmq
#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct Message {
    id: MessageId,
    source: ParticipantId,
    group: String,
    // todo The timestamp is now included in the Namespaced struct. Once the frontends adopted this change, remove the timestamp from Message
    timestamp: DateTime<Utc>,
    content: String,
}

fn group_routing_key(group: &str) -> String {
    format!("group.{}", group)
}

pub struct Chat {
    id: ParticipantId,
    room: SignalingRoomId,

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

        let groups = controller::block(move || db.get_groups_for_user(user_id))
            .await
            .context("controller::block failed")?
            .context("Failed to retrieve groups for user")?;

        for group in &groups {
            log::debug!("Group: {}", group.id);

            ctx.add_rabbitmq_binding(
                group_routing_key(&group.id),
                rabbitmq::current_room_exchange_name(ctx.room_id()),
                Default::default(),
            );
        }

        Ok(Self {
            id: ctx.participant_id(),
            room: ctx.room_id(),
            groups,
        })
    }

    async fn on_event(
        &mut self,
        mut ctx: ModuleContext<'_, Self>,
        event: Event<'_, Self>,
    ) -> Result<()> {
        let timestamp = ctx.timestamp();
        match event {
            Event::Joined {
                control_data: _,

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

                if self.is_in_group(&msg.group) {
                    let id = MessageId::new();

                    let stored_msg = StoredMessage {
                        id,
                        source: self.id,
                        timestamp: *timestamp,
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
                        rabbitmq::current_room_exchange_name(self.room),
                        group_routing_key(&msg.group),
                        Message {
                            id,
                            source: self.id,
                            group: msg.group,
                            timestamp: *timestamp,
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
            Event::RaiseHand | Event::LowerHand | Event::Ext(_) => {}
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
                        "Failed to acquire lock to cleanup group {:?}, {}",
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
