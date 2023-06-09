// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::{api::signaling::prelude::*, redis_wrapper::RedisConnection};
use actix_http::ws::CloseCode;
use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;
use types::core::{ParticipantId, RoomId, UserId};

pub mod incoming;
pub mod outgoing;
pub mod rabbitmq;
pub mod storage;

pub const NAMESPACE: &str = "moderation";

pub struct ModerationModule {
    room: SignalingRoomId,
    id: ParticipantId,
}

#[derive(Debug, Serialize)]
pub struct ModerationModuleFrontendData {
    waiting_room_enabled: bool,
    waiting_room_participants: Vec<control::outgoing::Participant>,
    raise_hands_enabled: bool,
}

async fn build_waiting_room_participants(
    redis_conn: &mut RedisConnection,
    room_id: RoomId,
    list: &Vec<ParticipantId>,
    waiting_room_state: control::outgoing::WaitingRoomState,
) -> Result<Vec<control::outgoing::Participant>> {
    let mut waiting_room = Vec::with_capacity(list.len());

    for id in list {
        let control_data =
            control::ControlData::from_redis(redis_conn, SignalingRoomId(room_id, None), *id)
                .await?;

        let module_data = HashMap::from([
            (control::NAMESPACE, serde_json::to_value(control_data)?),
            (
                "waiting_room_state",
                serde_json::to_value(&waiting_room_state)?,
            ),
        ]);

        waiting_room.push(control::outgoing::Participant {
            id: *id,
            module_data,
        });
    }

    Ok(waiting_room)
}

#[async_trait::async_trait(?Send)]
impl SignalingModule for ModerationModule {
    const NAMESPACE: &'static str = NAMESPACE;

    type Params = ();
    type Incoming = incoming::Message;
    type Outgoing = outgoing::Message;
    type RabbitMqMessage = rabbitmq::Message;
    type ExtEvent = ();
    type FrontendData = ModerationModuleFrontendData;
    type PeerFrontendData = ();

    async fn init(
        ctx: InitContext<'_, Self>,
        _params: &Self::Params,
        _protocol: &'static str,
    ) -> Result<Option<Self>> {
        Ok(Some(Self {
            room: ctx.room_id(),
            id: ctx.participant_id(),
        }))
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
                if ctx.role() == Role::Moderator {
                    let waiting_room_enabled =
                        storage::is_waiting_room_enabled(ctx.redis_conn(), self.room.room_id())
                            .await?;

                    let raise_hands_enabled =
                        storage::is_raise_hands_enabled(ctx.redis_conn(), self.room.room_id())
                            .await?;

                    let list =
                        storage::waiting_room_all(ctx.redis_conn(), self.room.room_id()).await?;
                    let mut waiting_room_participants = build_waiting_room_participants(
                        ctx.redis_conn(),
                        self.room.room_id(),
                        &list,
                        control::outgoing::WaitingRoomState::Waiting,
                    )
                    .await?;

                    let list =
                        storage::waiting_room_accepted_all(ctx.redis_conn(), self.room.room_id())
                            .await?;
                    let mut accepted_waiting_room_participants = build_waiting_room_participants(
                        ctx.redis_conn(),
                        self.room.room_id(),
                        &list,
                        control::outgoing::WaitingRoomState::Accepted,
                    )
                    .await?;

                    waiting_room_participants.append(&mut accepted_waiting_room_participants);

                    *frontend_data = Some(ModerationModuleFrontendData {
                        waiting_room_enabled,
                        waiting_room_participants,
                        raise_hands_enabled,
                    });
                }
            }
            Event::Leaving => {}
            Event::RaiseHand => {}
            Event::LowerHand => {}
            Event::ParticipantJoined(_, _) => {}
            Event::ParticipantLeft(_) => {}
            Event::ParticipantUpdated(_, _) => {}
            Event::WsMessage(incoming::Message::Ban(incoming::Target { target })) => {
                if ctx.role() != Role::Moderator {
                    return Ok(());
                }

                let user_id: Option<UserId> =
                    control::storage::get_attribute(ctx.redis_conn(), self.room, target, "user_id")
                        .await?;

                if let Some(user_id) = user_id {
                    storage::ban_user(ctx.redis_conn(), self.room.room_id(), user_id).await?;
                } else {
                    ctx.ws_send(outgoing::Message::Error(outgoing::Error::CannotBanGuest));
                    return Ok(());
                }

                ctx.rabbitmq_publish(
                    control::rabbitmq::current_room_exchange_name(self.room),
                    control::rabbitmq::room_participant_routing_key(target),
                    rabbitmq::Message::Banned(target),
                );
            }
            Event::WsMessage(incoming::Message::Kick(incoming::Target { target })) => {
                if ctx.role() != Role::Moderator {
                    return Ok(());
                }

                ctx.rabbitmq_publish(
                    control::rabbitmq::current_room_exchange_name(self.room),
                    control::rabbitmq::room_participant_routing_key(target),
                    rabbitmq::Message::Kicked(target),
                );
            }
            Event::WsMessage(incoming::Message::EnableWaitingRoom) => {
                if ctx.role() != Role::Moderator {
                    return Ok(());
                }

                storage::set_waiting_room_enabled(ctx.redis_conn(), self.room.room_id(), true)
                    .await?;

                ctx.rabbitmq_publish(
                    breakout::rabbitmq::global_exchange_name(self.room.room_id()),
                    control::rabbitmq::room_all_routing_key().into(),
                    rabbitmq::Message::WaitingRoomEnableUpdated,
                );
            }
            Event::WsMessage(incoming::Message::DisableWaitingRoom) => {
                if ctx.role() != Role::Moderator {
                    return Ok(());
                }

                storage::set_waiting_room_enabled(ctx.redis_conn(), self.room.room_id(), false)
                    .await?;

                ctx.rabbitmq_publish(
                    breakout::rabbitmq::global_exchange_name(self.room.room_id()),
                    control::rabbitmq::room_all_routing_key().into(),
                    rabbitmq::Message::WaitingRoomEnableUpdated,
                );
            }
            Event::WsMessage(incoming::Message::Accept(incoming::Target { target })) => {
                if ctx.role() != Role::Moderator {
                    return Ok(());
                }

                if !storage::waiting_room_contains(ctx.redis_conn(), self.room.room_id(), target)
                    .await?
                {
                    // TODO return error
                    return Ok(());
                }

                storage::waiting_room_accepted_add(ctx.redis_conn(), self.room.room_id(), target)
                    .await?;
                storage::waiting_room_remove(ctx.redis_conn(), self.room.room_id(), target).await?;

                ctx.rabbitmq_publish_control(
                    control::rabbitmq::current_room_exchange_name(self.room),
                    control::rabbitmq::room_participant_routing_key(target),
                    control::rabbitmq::Message::Accepted(target),
                );
            }
            Event::WsMessage(incoming::Message::ResetRaisedHands) => {
                if ctx.role() != Role::Moderator {
                    return Ok(());
                }

                ctx.rabbitmq_publish_control(
                    control::rabbitmq::current_room_exchange_name(self.room),
                    control::rabbitmq::room_all_routing_key().to_string(),
                    control::rabbitmq::Message::ResetRaisedHands { issued_by: self.id },
                );
            }

            Event::WsMessage(incoming::Message::EnableRaiseHands) => {
                if ctx.role() != Role::Moderator {
                    return Ok(());
                }

                storage::set_raise_hands_enabled(ctx.redis_conn(), self.room.room_id(), true)
                    .await?;

                ctx.rabbitmq_publish_control(
                    control::rabbitmq::current_room_exchange_name(self.room),
                    control::rabbitmq::room_all_routing_key().to_string(),
                    control::rabbitmq::Message::EnableRaiseHands { issued_by: self.id },
                );
            }

            Event::WsMessage(incoming::Message::DisableRaiseHands) => {
                if ctx.role() != Role::Moderator {
                    return Ok(());
                }

                storage::set_raise_hands_enabled(ctx.redis_conn(), self.room.room_id(), false)
                    .await?;

                ctx.rabbitmq_publish_control(
                    control::rabbitmq::current_room_exchange_name(self.room),
                    control::rabbitmq::room_all_routing_key().to_string(),
                    control::rabbitmq::Message::DisableRaiseHands { issued_by: self.id },
                );
            }

            Event::RabbitMq(rabbitmq::Message::Banned(participant)) => {
                if self.id == participant {
                    ctx.ws_send(outgoing::Message::Banned);
                    ctx.exit(Some(CloseCode::Normal));
                }
            }
            Event::RabbitMq(rabbitmq::Message::Kicked(participant)) => {
                if self.id == participant {
                    ctx.ws_send(outgoing::Message::Kicked);
                    ctx.exit(Some(CloseCode::Normal));
                }
            }
            Event::RabbitMq(rabbitmq::Message::JoinedWaitingRoom(id)) => {
                if self.id == id {
                    return Ok(());
                }

                let control_data =
                    control::ControlData::from_redis(ctx.redis_conn(), self.room, id).await?;

                let module_data =
                    HashMap::from([(control::NAMESPACE, serde_json::to_value(control_data)?)]);

                ctx.ws_send(outgoing::Message::JoinedWaitingRoom(
                    control::outgoing::Participant { id, module_data },
                ));
            }
            Event::RabbitMq(rabbitmq::Message::LeftWaitingRoom(id)) => {
                if self.id == id {
                    return Ok(());
                }

                ctx.ws_send(outgoing::Message::LeftWaitingRoom(
                    control::outgoing::AssociatedParticipant { id },
                ));
            }
            Event::RabbitMq(rabbitmq::Message::WaitingRoomEnableUpdated) => {
                let enabled =
                    storage::is_waiting_room_enabled(ctx.redis_conn(), self.room.room_id()).await?;

                if enabled {
                    ctx.ws_send(outgoing::Message::WaitingRoomEnabled);
                } else {
                    ctx.ws_send(outgoing::Message::WaitingRoomDisabled);
                }
            }
            Event::Ext(_) => unreachable!(),
        }

        Ok(())
    }

    async fn on_destroy(self, mut ctx: DestroyContext<'_>) {
        if ctx.destroy_room() {
            if let Err(e) = storage::delete_bans(ctx.redis_conn(), self.room.room_id()).await {
                log::error!("Failed to clean up bans list {}", e);
            }

            if let Err(e) =
                storage::delete_waiting_room_enabled(ctx.redis_conn(), self.room.room_id()).await
            {
                log::error!("Failed to clean up waiting room enabled flag {}", e);
            }

            if let Err(e) =
                storage::delete_raise_hands_enabled(ctx.redis_conn(), self.room.room_id()).await
            {
                log::error!("Failed to clean up raise hands enabled flag {}", e);
            }

            if let Err(e) =
                storage::delete_waiting_room(ctx.redis_conn(), self.room.room_id()).await
            {
                log::error!("Failed to clean up waiting room list {}", e);
            }

            if let Err(e) =
                storage::delete_waiting_room_accepted(ctx.redis_conn(), self.room.room_id()).await
            {
                log::error!("Failed to clean up accepted waiting room list {}", e);
            }
        }
    }
}
