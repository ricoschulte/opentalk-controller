use crate::api::signaling::prelude::*;
use actix_http::ws::CloseCode;
use anyhow::Result;
use controller_shared::ParticipantId;
use db_storage::users::UserId;

pub mod incoming;
pub mod outgoing;
pub mod rabbitmq;
pub mod storage;

pub const NAMESPACE: &str = "moderation";

pub struct ModerationModule {
    room: SignalingRoomId,
    id: ParticipantId,
    role: Role,
}

#[async_trait::async_trait(?Send)]
impl SignalingModule for ModerationModule {
    const NAMESPACE: &'static str = NAMESPACE;

    type Params = ();
    type Incoming = incoming::Message;
    type Outgoing = outgoing::Message;
    type RabbitMqMessage = rabbitmq::Message;
    type ExtEvent = ();
    type FrontendData = ();
    type PeerFrontendData = ();

    async fn init(
        ctx: InitContext<'_, Self>,
        _params: &Self::Params,
        _protocol: &'static str,
    ) -> Result<Option<Self>> {
        Ok(Some(Self {
            room: ctx.room_id(),
            id: ctx.participant_id(),
            role: ctx.role(),
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
                frontend_data: _,
                participants: _,
            } => {}
            Event::Leaving => {}
            Event::RaiseHand => {}
            Event::LowerHand => {}
            Event::ParticipantJoined(_, _) => {}
            Event::ParticipantLeft(_) => {}
            Event::ParticipantUpdated(_, _) => {}
            Event::WsMessage(incoming::Message::Ban(incoming::Target { target })) => {
                if self.role != Role::Moderator {
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
                if self.role != Role::Moderator {
                    return Ok(());
                }

                ctx.rabbitmq_publish(
                    control::rabbitmq::current_room_exchange_name(self.room),
                    control::rabbitmq::room_participant_routing_key(target),
                    rabbitmq::Message::Kicked(target),
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
            Event::Ext(_) => unreachable!(),
        }

        Ok(())
    }

    async fn on_destroy(self, mut ctx: DestroyContext<'_>) {
        if ctx.destroy_room() {
            if let Err(e) = storage::delete_bans(ctx.redis_conn(), self.room.room_id()).await {
                log::error!("Failed to clean up bans list {}", e);
            }
        }
    }
}
