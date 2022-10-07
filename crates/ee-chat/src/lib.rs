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
use controller::prelude::*;
use controller_shared::ParticipantId;
use database::Db;
use db_storage::groups::{Group, GroupId};
use db_storage::users::UserId;
use r3dlock::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use storage::StoredMessage;
mod storage;

/// Message received from websocket
#[derive(Debug, Deserialize)]
pub struct IncomingWsMessage {
    group: String,
    content: String,
}

/// Message sent via websocket and rabbitmq
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Message {
    id: MessageId,
    source: ParticipantId,
    group: String,
    // todo The timestamp is now included in the Namespaced struct. Once the frontends adopted this change, remove the timestamp from Message
    timestamp: DateTime<Utc>,
    content: String,
}

fn group_routing_key(group_id: GroupId) -> String {
    format!("group.{}", group_id)
}

pub struct Chat {
    id: ParticipantId,
    room: SignalingRoomId,

    db: Arc<Db>,

    groups: Vec<Group>,
}

impl Chat {
    fn get_group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|group| group.name == name)
    }
}

#[derive(Serialize)]
pub struct FrontendDataEntry {
    name: String,
    history: Vec<StoredMessage>,
}

#[derive(Serialize)]
pub struct PeerFrontendData {
    groups: Vec<String>,
}

#[async_trait::async_trait(?Send)]
impl SignalingModule for Chat {
    const NAMESPACE: &'static str = "ee_chat";
    type Params = ();
    type Incoming = IncomingWsMessage;
    type Outgoing = Message;
    type RabbitMqMessage = Message;
    type ExtEvent = ();
    type FrontendData = Vec<FrontendDataEntry>;
    type PeerFrontendData = PeerFrontendData;

    async fn init(
        mut ctx: InitContext<'_, Self>,
        _params: &Self::Params,
        _protocol: &'static str,
    ) -> Result<Option<Self>> {
        if let Participant::User(user) = ctx.participant() {
            let user_id = user.id;
            let db = ctx.db().clone();

            let groups = controller::block(move || {
                let mut conn = db.get_conn()?;

                Group::get_all_for_user(&mut conn, user_id)
            })
            .await?
            .context("Failed to retrieve groups for user")?;

            for group in &groups {
                log::debug!("Group: {}", group.id);

                ctx.add_rabbitmq_binding(
                    group_routing_key(group.id),
                    rabbitmq::current_room_exchange_name(ctx.room_id()),
                    Default::default(),
                );
            }

            Ok(Some(Self {
                id: ctx.participant_id(),
                room: ctx.room_id(),
                db: ctx.db().clone(),
                groups,
            }))
        } else {
            Ok(None)
        }
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
                participants,
            } => {
                // ==== Collect group message history ====
                let mut group_messages = Vec::new();

                for group in &self.groups {
                    storage::add_participant_to_set(ctx.redis_conn(), self.room, group.id, self.id)
                        .await?;

                    let history =
                        storage::get_group_chat_history(ctx.redis_conn(), self.room, group.id)
                            .await?;

                    group_messages.push(FrontendDataEntry {
                        name: group.name.clone(),
                        history,
                    });
                }

                *frontend_data = Some(group_messages);

                // ==== Find other participant in our group ====
                let participant_ids: Vec<ParticipantId> =
                    participants.iter().map(|(id, _)| *id).collect();

                // Get all user_ids for each participant in the room
                let user_ids: Vec<Option<UserId>> =
                    control::storage::get_attribute_for_participants(
                        ctx.redis_conn(),
                        self.room,
                        "user_id",
                        &participant_ids,
                    )
                    .await?;

                // Filter out guest/bots and map each user-id to a participant id
                let participant_user_mappings: Vec<(UserId, ParticipantId)> = user_ids
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, opt_user_id)| {
                        opt_user_id.map(|user_id| (user_id, participant_ids[i]))
                    })
                    .collect();

                let db = self.db.clone();
                let self_groups = self.groups.clone();

                // Inquire the database about each user's groups
                let participant_to_common_groups_mappings =
                    controller::block(move || -> Result<Vec<(ParticipantId, Vec<String>)>> {
                        let mut conn = db.get_conn()?;

                        let mut participant_to_common_groups_mappings = vec![];

                        for (user_id, participant_id) in participant_user_mappings {
                            // Get the users groups
                            let groups = Group::get_all_for_user(&mut conn, user_id)?;

                            // Intersect our groups and the groups of the user and collect their id/name
                            // as strings into a set
                            let common_groups = self_groups
                                .iter()
                                .filter(|self_group| groups.contains(self_group))
                                .map(|group| group.name.clone())
                                .collect();

                            participant_to_common_groups_mappings
                                .push((participant_id, common_groups));
                        }

                        Ok(participant_to_common_groups_mappings)
                    })
                    .await??;

                // Iterate over the result of the controller::block call and insert the common groups
                // into the PeerFrontendData
                for (participant_id, common_groups) in participant_to_common_groups_mappings {
                    if let Some(participant_frontend_data) = participants.get_mut(&participant_id) {
                        *participant_frontend_data = Some(PeerFrontendData {
                            groups: common_groups,
                        });
                    } else {
                        log::error!("Got invalid participant id")
                    }
                }
            }
            Event::Leaving => {}
            Event::ParticipantJoined(participant_id, peer_frontend_data) => {
                // Get user id of the joined participant
                let user_id: Option<UserId> = control::storage::get_attribute(
                    ctx.redis_conn(),
                    self.room,
                    participant_id,
                    "user_id",
                )
                .await?;

                if let Some(user_id) = user_id {
                    let db = self.db.clone();
                    let self_groups = self.groups.clone();

                    let common_groups = controller::block(move || -> Result<Vec<String>> {
                        // Get the user's groups
                        let mut conn = db.get_conn()?;

                        let groups = Group::get_all_for_user(&mut conn, user_id)?;

                        // Intersect our groups and the groups of the user and collect their id/name
                        // as strings into a set
                        let common_groups = self_groups
                            .iter()
                            .filter(|self_group| groups.contains(self_group))
                            .map(|group| group.name.clone())
                            .collect();

                        Ok(common_groups)
                    })
                    .await??;

                    *peer_frontend_data = Some(PeerFrontendData {
                        groups: common_groups,
                    })
                }
            }
            Event::ParticipantLeft(_) => {}
            Event::ParticipantUpdated(_, _) => {}
            Event::WsMessage(mut msg) => {
                // Discard empty messages
                if msg.content.is_empty() {
                    return Ok(());
                }

                let chat_enabled =
                    chat::is_chat_enabled(ctx.redis_conn(), self.room.room_id()).await?;

                if !chat_enabled {
                    // TODO: send error as done in the chat module
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

                if let Some(group) = self.get_group(&msg.group) {
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
                        group.id,
                        &stored_msg,
                    )
                    .await?;

                    ctx.rabbitmq_publish(
                        rabbitmq::current_room_exchange_name(self.room),
                        group_routing_key(group.id),
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
                if self.get_group(&msg.group).is_some() {
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
                group: group.id,
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
                group.id,
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
                    storage::delete_group_chat_history(ctx.redis_conn(), self.room, group.id).await
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
