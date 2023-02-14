// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use controller::prelude::anyhow::Result;
use controller::prelude::chrono::{self, Utc};
use controller::prelude::futures::stream::once;
use controller::prelude::futures::FutureExt;
use controller::prelude::log;
use controller::prelude::redis;
use controller::prelude::redis::FromRedisValue;
use controller::prelude::redis::RedisResult;
use controller::prelude::tokio::time::sleep;
use controller::prelude::uuid::Uuid;
use controller::prelude::Event;
use controller::prelude::Timestamp;
use controller::prelude::{
    async_trait, control, InitContext, ModuleContext, Role, SignalingModule, SignalingRoomId,
};
use controller_shared::ParticipantId;
use outgoing::StopKind;
use redis_args::ToRedisArgs;
use serde::Deserialize;
use serde::Serialize;
use std::fmt;
use std::str::from_utf8;
use std::str::FromStr;
use storage::ready_status::ReadyStatus;

pub mod incoming;
pub mod outgoing;
pub mod rabbitmq;
mod storage;

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, ToRedisArgs)]
#[to_redis_args(fmt)]
pub struct TimerId(pub Uuid);

impl FromRedisValue for TimerId {
    fn from_redis_value(v: &redis::Value) -> RedisResult<Self> {
        match v {
            redis::Value::Data(bytes) => {
                Uuid::from_str(from_utf8(bytes)?).map(Self).map_err(|_| {
                    redis::RedisError::from((
                        redis::ErrorKind::TypeError,
                        "invalid data for TimerId",
                    ))
                })
            }
            _ => RedisResult::Err(redis::RedisError::from((
                redis::ErrorKind::TypeError,
                "invalid data type for TimerId",
            ))),
        }
    }
}

impl fmt::Display for TimerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The expiry event for a timer
pub struct ExpiredEvent {
    timer_id: TimerId,
}

pub struct Timer {
    pub room_id: SignalingRoomId,
    pub participant_id: ParticipantId,
}

#[async_trait::async_trait(?Send)]
impl SignalingModule for Timer {
    const NAMESPACE: &'static str = "timer";

    type Params = ();

    type Incoming = incoming::Message;

    type Outgoing = outgoing::Message;

    type RabbitMqMessage = rabbitmq::Event;

    type ExtEvent = ExpiredEvent;

    type FrontendData = outgoing::Started;

    type PeerFrontendData = ReadyStatus;

    async fn init(
        ctx: InitContext<'_, Self>,
        _params: &Self::Params,
        _protocol: &'static str,
    ) -> Result<Option<Self>> {
        Ok(Some(Self {
            room_id: ctx.room_id(),
            participant_id: ctx.participant_id(),
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
                participants,
            } => {
                let timer = storage::timer::get(ctx.redis_conn(), self.room_id).await?;

                let timer = match timer {
                    Some(timer) => timer,
                    None => return Ok(()),
                };

                *frontend_data = Some(outgoing::Started {
                    timer_id: timer.id,
                    started_at: timer.started_at,
                    kind: timer.kind,
                    style: timer.style,
                    title: timer.title,
                    ready_check_enabled: timer.ready_check_enabled,
                });

                if let outgoing::Kind::Countdown { ends_at } = timer.kind {
                    ctx.add_event_stream(once(
                        sleep(
                            ends_at
                                .signed_duration_since(Utc::now())
                                .to_std()
                                .unwrap_or_default(),
                        )
                        .map(move |_| ExpiredEvent { timer_id: timer.id }),
                    ));
                }

                for (participant_id, status) in participants {
                    let ready_status =
                        storage::ready_status::get(ctx.redis_conn(), self.room_id, *participant_id)
                            .await?;

                    *status = ready_status;
                }
            }
            Event::Leaving => {
                storage::ready_status::delete(ctx.redis_conn(), self.room_id, self.participant_id)
                    .await?;
            }
            Event::WsMessage(msg) => self.handle_ws_message(&mut ctx, msg).await?,
            Event::RabbitMq(event) => {
                self.handle_rmq_message(&mut ctx, event).await?;
            }
            Event::Ext(expired) => {
                if let Some(timer) = storage::timer::get(ctx.redis_conn(), self.room_id).await? {
                    if timer.id == expired.timer_id {
                        self.stop_current_timer(&mut ctx, StopKind::Expired, None)
                            .await?;
                    }
                }
            }
            // Unused events
            Event::RaiseHand
            | Event::LowerHand
            | Event::ParticipantJoined(..)
            | Event::ParticipantUpdated(..)
            | Event::ParticipantLeft(_) => (),
        }

        Ok(())
    }

    async fn on_destroy(self, _ctx: controller::prelude::DestroyContext<'_>) {}
}

impl Timer {
    /// Handle incoming websocket messages
    async fn handle_ws_message(
        &self,
        ctx: &mut ModuleContext<'_, Self>,
        msg: incoming::Message,
    ) -> Result<()> {
        match msg {
            incoming::Message::Start(start) => {
                if ctx.role() != Role::Moderator {
                    ctx.ws_send(outgoing::Message::Error(
                        outgoing::Error::InsufficientPermissions,
                    ));
                    return Ok(());
                }

                let timer_id = TimerId(Uuid::new_v4());

                let started_at = ctx.timestamp();

                // determine the end time at the start of the timer to later calculate the remaining duration for joining participants
                let kind = match start.kind {
                    incoming::Kind::Countdown { duration } => {
                        let duration = match duration.try_into() {
                            Ok(duration) => duration,
                            Err(_) => {
                                ctx.ws_send(outgoing::Message::Error(
                                    outgoing::Error::InvalidDuration,
                                ));

                                return Ok(());
                            }
                        };

                        match started_at.checked_add_signed(chrono::Duration::seconds(duration)) {
                            Some(ends_at) => outgoing::Kind::Countdown {
                                ends_at: Timestamp::from(ends_at),
                            },
                            None => {
                                log::error!("DateTime overflow in timer module");
                                ctx.ws_send(outgoing::Message::Error(
                                    outgoing::Error::InvalidDuration,
                                ));

                                return Ok(());
                            }
                        }
                    }
                    incoming::Kind::Stopwatch => outgoing::Kind::Stopwatch,
                };

                let timer = storage::timer::Timer {
                    id: timer_id,
                    created_by: self.participant_id,
                    started_at,
                    kind,
                    style: start.style.clone(),
                    title: start.title.clone(),
                    ready_check_enabled: start.enable_ready_check,
                };

                if !storage::timer::set_if_not_exists(ctx.redis_conn(), self.room_id, &timer)
                    .await?
                {
                    ctx.ws_send(outgoing::Message::Error(
                        outgoing::Error::TimerAlreadyRunning,
                    ));
                    return Ok(());
                }

                let started = outgoing::Started {
                    timer_id,
                    started_at,
                    kind,
                    style: start.style,
                    title: start.title,
                    ready_check_enabled: start.enable_ready_check,
                };

                ctx.rabbitmq_publish(
                    control::rabbitmq::current_room_exchange_name(self.room_id),
                    control::rabbitmq::room_all_routing_key().into(),
                    rabbitmq::Event::Start(started),
                );
            }
            incoming::Message::Stop(stop) => {
                if ctx.role() != Role::Moderator {
                    ctx.ws_send(outgoing::Message::Error(
                        outgoing::Error::InsufficientPermissions,
                    ));
                    return Ok(());
                }

                match storage::timer::get(ctx.redis_conn(), self.room_id).await? {
                    Some(timer) => {
                        if timer.id != stop.timer_id {
                            // Invalid timer id
                            return Ok(());
                        }
                    }
                    None => {
                        // no timer active
                        return Ok(());
                    }
                }

                self.stop_current_timer(
                    ctx,
                    StopKind::ByModerator(self.participant_id),
                    stop.reason,
                )
                .await?;
            }
            incoming::Message::UpdateReadyStatus(update_ready_status) => {
                if let Some(timer) = storage::timer::get(ctx.redis_conn(), self.room_id).await? {
                    if timer.ready_check_enabled && timer.id == update_ready_status.timer_id {
                        storage::ready_status::set(
                            ctx.redis_conn(),
                            self.room_id,
                            self.participant_id,
                            update_ready_status.status,
                        )
                        .await?;

                        ctx.rabbitmq_publish(
                            control::rabbitmq::current_room_exchange_name(self.room_id),
                            control::rabbitmq::room_all_routing_key().into(),
                            rabbitmq::Event::UpdateReadyStatus(rabbitmq::UpdateReadyStatus {
                                timer_id: timer.id,
                                participant_id: self.participant_id,
                            }),
                        );
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_rmq_message(
        &self,
        ctx: &mut ModuleContext<'_, Self>,
        event: rabbitmq::Event,
    ) -> Result<()> {
        match event {
            rabbitmq::Event::Start(started) => {
                if let outgoing::Kind::Countdown { ends_at } = started.kind {
                    ctx.add_event_stream(once(
                        sleep(
                            ends_at
                                .signed_duration_since(Utc::now())
                                .to_std()
                                .unwrap_or_default(),
                        )
                        .map(move |_| ExpiredEvent {
                            timer_id: started.timer_id,
                        }),
                    ));
                }

                ctx.ws_send(outgoing::Message::Started(started));
            }
            rabbitmq::Event::Stop(stopped) => {
                // remove the participants ready status when receiving 'stopped'
                storage::ready_status::delete(ctx.redis_conn(), self.room_id, self.participant_id)
                    .await?;

                ctx.ws_send(outgoing::Message::Stopped(stopped));
            }
            rabbitmq::Event::UpdateReadyStatus(update_ready_status) => {
                if let Some(ready_status) = storage::ready_status::get(
                    ctx.redis_conn(),
                    self.room_id,
                    update_ready_status.participant_id,
                )
                .await?
                {
                    ctx.ws_send(outgoing::Message::UpdatedReadyStatus(
                        outgoing::UpdatedReadyStatus {
                            timer_id: update_ready_status.timer_id,
                            participant_id: update_ready_status.participant_id,
                            status: ready_status.ready_status,
                        },
                    ))
                }
            }
        }

        Ok(())
    }

    /// Stop the current timer and publish a [`outgoing::Stopped`] message to all participants
    ///
    /// Does not send the [`outgoing::Stopped`] message when there is no timer running
    async fn stop_current_timer(
        &self,
        ctx: &mut ModuleContext<'_, Self>,
        reason: StopKind,
        message: Option<String>,
    ) -> Result<()> {
        let timer = match storage::timer::delete(ctx.redis_conn(), self.room_id).await? {
            Some(timer) => timer,
            // there was no key to delete because the timer was not running
            None => return Ok(()),
        };

        ctx.rabbitmq_publish(
            control::rabbitmq::current_room_exchange_name(self.room_id),
            control::rabbitmq::room_all_routing_key().into(),
            rabbitmq::Event::Stop(outgoing::Stopped {
                timer_id: timer.id,
                kind: reason,
                reason: message,
            }),
        );

        Ok(())
    }
}

pub fn register(controller: &mut controller::Controller) {
    controller.signaling.add_module::<Timer>(());
}
