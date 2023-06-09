// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use super::actor::WebSocketActor;
use super::modules::{
    AnyStream, DynBroadcastEvent, DynEventCtx, DynTargetedEvent, Modules, NoSuchModuleError,
};
use super::{
    DestroyContext, NamespacedCommand, NamespacedEvent, RabbitMqBinding, RabbitMqExchange,
    RabbitMqPublish, Timestamp,
};
use crate::api;
use crate::api::signaling::metrics::SignalingMetrics;
use crate::api::signaling::prelude::control::outgoing::JoinBlockedReason;
use crate::api::signaling::prelude::*;
use crate::api::signaling::resumption::{ResumptionTokenKeepAlive, ResumptionTokenUsed};
use crate::api::signaling::ws::actor::WsCommand;
use crate::api::signaling::ws_modules::control::outgoing::Participant;
use crate::api::signaling::ws_modules::control::storage::ParticipantIdRunnerLock;
use crate::api::signaling::ws_modules::control::{
    incoming, outgoing, rabbitmq, storage, ControlData, NAMESPACE,
};
use crate::api::signaling::{Role, SignalingRoomId};
use crate::api::v1::tariffs::TariffResource;
use crate::redis_wrapper::RedisConnection;
use crate::storage::ObjectStorage;
use actix::Addr;
use actix_http::ws::{CloseCode, CloseReason, Message};
use actix_web_actors::ws;
use anyhow::{bail, Context, Result};
use chrono::TimeZone;
use controller_shared::settings::SharedSettings;
use database::Db;
use db_storage::rooms::Room;
use db_storage::tariffs::Tariff;
use db_storage::users::User;
use futures::stream::SelectAll;
use futures::Future;
use itertools::Itertools;
use kustos::Authz;
use lapin::message::DeliveryResult;
use lapin::options::{ExchangeDeclareOptions, QueueDeclareOptions};
use lapin::{BasicProperties, ExchangeKind};
use lapin_pool::RabbitMqChannel;
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashMap;
use std::future;
use std::mem::replace;
use std::ops::ControlFlow;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};
use tokio::time::{interval, sleep};
use tokio_stream::StreamExt;
use types::core::{BreakoutRoomId, ParticipantId, ParticipationKind, UserId};
use uuid::Uuid;

mod sip;

// The expiry in seconds for the `skip_waiting_room` key in Redis
const SKIP_WAITING_ROOM_KEY_EXPIRY: usize = 120;
const SKIP_WAITING_ROOM_KEY_REFRESH_INTERVAL: u64 = 60;

/// Builder to the runner type.
///
/// Passed into [`ModuleBuilder::build`](super::modules::ModuleBuilder::build) function to create an [`InitContext`](super::InitContext).
pub struct Builder {
    runner_id: Uuid,
    pub(super) id: ParticipantId,
    resuming: bool,
    pub(super) room: Room,
    pub(super) breakout_room: Option<BreakoutRoomId>,
    pub(super) participant: api::Participant<User>,
    pub(super) role: Role,
    pub(super) protocol: &'static str,
    pub(super) metrics: Arc<SignalingMetrics>,
    pub(super) modules: Modules,
    pub(super) rabbitmq_exchanges: Vec<RabbitMqExchange>,
    pub(super) rabbitmq_bindings: Vec<RabbitMqBinding>,
    pub(super) events: SelectAll<AnyStream>,
    pub(super) db: Arc<Db>,
    pub(super) storage: Arc<ObjectStorage>,
    pub(super) authz: Arc<Authz>,
    pub(super) redis_conn: RedisConnection,
    pub(super) rabbitmq_channel: RabbitMqChannel,
    resumption_keep_alive: ResumptionTokenKeepAlive,
}

impl Builder {
    /// Abort the building process and destroy all already built modules
    #[tracing::instrument(skip(self))]
    pub async fn abort(mut self) {
        let ctx = DestroyContext {
            redis_conn: &mut self.redis_conn,
            // We haven't joined yet
            destroy_room: false,
        };

        self.modules.destroy(ctx).await
    }

    async fn aquire_participant_id(&mut self) -> Result<()> {
        let key = ParticipantIdRunnerLock { id: self.id };
        let runner_id = self.runner_id.to_string();

        // Try for up to 10 secs to aquire the key
        for _ in 0..10 {
            let value: redis::Value = redis::cmd("SET")
                .arg(&key)
                .arg(&runner_id)
                .arg("NX")
                .query_async(&mut self.redis_conn)
                .await?;

            match value {
                redis::Value::Nil => sleep(Duration::from_secs(1)).await,
                redis::Value::Okay => return Ok(()),
                _ => bail!(
                    "got unexpected value while acquiring runner id, value={:?}",
                    value
                ),
            }
        }

        bail!("failed to aquire runner id");
    }

    /// Build to runner from the data inside the builder and provided websocket
    #[tracing::instrument(err, skip_all)]
    pub async fn build(
        mut self,
        to_ws_actor: Addr<WebSocketActor>,
        from_ws_actor: mpsc::UnboundedReceiver<Message>,
        shutdown_sig: broadcast::Receiver<()>,
        settings: SharedSettings,
    ) -> Result<Runner> {
        self.aquire_participant_id().await?;
        // ==== CONTROL MODULE DECLARATIONS ====

        let room_id = SignalingRoomId(self.room.id, self.breakout_room);

        // The name of the room exchange
        let room_exchange = rabbitmq::current_room_exchange_name(room_id);

        // The name of the room's global exchange
        let global_room_exchange = breakout::rabbitmq::global_exchange_name(self.room.id);

        // Routing key to receive messages directed to all participants
        let all_routing_key = rabbitmq::room_all_routing_key();

        // Routing key which is needed to receive messages to this participant
        let self_routing_key = rabbitmq::room_participant_routing_key(self.id);

        // ==== ROOM EXCHANGE BINDINGS

        // Create a topic exchange for global messages inside the room
        // Insert at beginning to ensure that later bindings reference an existing exchange
        self.rabbitmq_exchanges.insert(
            0,
            RabbitMqExchange {
                name: room_exchange.clone(),
                kind: ExchangeKind::Topic,
                options: Default::default(),
            },
        );

        // Add binding to messages directed to all participants
        self.rabbitmq_bindings.push(RabbitMqBinding {
            routing_key: all_routing_key.into(),
            exchange: room_exchange.clone(),
            options: Default::default(),
        });

        // Add binding to messages directed to self_key (the participant)
        self.rabbitmq_bindings.push(RabbitMqBinding {
            routing_key: self_routing_key.clone(),
            exchange: room_exchange.clone(),
            options: Default::default(),
        });

        // ==== GLOBAL ROOM EXCHANGE BINDINGS

        // Bind ourself to global exchange to communicate across breakout boundaries
        self.rabbitmq_exchanges.insert(
            1,
            RabbitMqExchange {
                name: breakout::rabbitmq::global_exchange_name(self.room.id),
                kind: ExchangeKind::Topic,
                options: ExchangeDeclareOptions {
                    auto_delete: true,
                    ..Default::default()
                },
            },
        );

        self.rabbitmq_bindings.push(RabbitMqBinding {
            routing_key: all_routing_key.into(),
            exchange: global_room_exchange.clone(),
            options: Default::default(),
        });

        self.rabbitmq_bindings.push(RabbitMqBinding {
            routing_key: self_routing_key.clone(),
            exchange: global_room_exchange.clone(),
            options: Default::default(),
        });

        // ==== BEGIN GENERIC SETUP ====

        // Create the queue for this participant
        let queue_options = QueueDeclareOptions {
            auto_delete: true,
            ..Default::default()
        };

        let queue = self
            .rabbitmq_channel
            .queue_declare(
                &format!("k3k-controller.runner.{}", self.runner_id),
                queue_options,
                Default::default(),
            )
            .await
            .context("Failed to create rabbitmq queue for websocket task")?;

        for RabbitMqExchange {
            name,
            kind,
            options,
        } in self.rabbitmq_exchanges
        {
            log::debug!(
                "Declaring rabbitmq exchange kind={:?} name={:?}",
                kind,
                name
            );

            self.rabbitmq_channel
                .exchange_declare(&name, kind, options, Default::default())
                .await?;
        }

        for RabbitMqBinding {
            routing_key,
            exchange,
            options,
        } in self.rabbitmq_bindings
        {
            log::debug!(
                "Creating queue binding: name={:?} routing_key={:?} exchange={:?}",
                queue.name().as_str(),
                routing_key,
                exchange
            );

            self.rabbitmq_channel
                .queue_bind(
                    queue.name().as_str(),
                    &exchange,
                    &routing_key,
                    options,
                    Default::default(),
                )
                .await?;
        }
        if let api::Participant::User(user) = &self.participant {
            // Routing key which is needed to receive messages to this participant
            let self_user_routing_key = rabbitmq::room_user_routing_key(user.id);

            // Binding outside the loop since the routing key contains the user-id
            // and thus should not appear in the logs
            self.rabbitmq_channel
                .queue_bind(
                    queue.name().as_str(),
                    &room_exchange,
                    &self_user_routing_key,
                    Default::default(),
                    Default::default(),
                )
                .await?;

            self.rabbitmq_channel
                .queue_bind(
                    queue.name().as_str(),
                    &global_room_exchange,
                    &self_user_routing_key,
                    Default::default(),
                    Default::default(),
                )
                .await?;
        }

        let consumer = self
            .rabbitmq_channel
            .basic_consume(
                queue.name().as_str(),
                // Visual aid: set participant key as consumer tag
                &self_routing_key,
                Default::default(),
                Default::default(),
            )
            .await?;

        self.resumption_keep_alive
            .set_initial(&mut self.redis_conn)
            .await?;

        Ok(Runner {
            runner_id: self.runner_id,
            id: self.id,
            resuming: self.resuming,
            room: self.room,
            room_id,
            participant: self.participant,
            role: self.role,
            state: RunnerState::None,
            ws: Ws {
                to_actor: to_ws_actor,
                from_actor: from_ws_actor,
                state: State::Open,
            },
            modules: self.modules,
            events: self.events,
            metrics: self.metrics,
            db: self.db,
            redis_conn: self.redis_conn,
            consumer,
            consumer_delegated: false,
            rabbitmq_channel: self.rabbitmq_channel,
            room_exchange,
            resumption_keep_alive: self.resumption_keep_alive,
            shutdown_sig,
            exit: false,
            settings,
            time_limit_future: Box::pin(future::pending()),
        })
    }
}

/// The session runner
///
/// As root of the session-task it is responsible to drive the module,
/// manage setup and teardown of redis storage, RabbitMQ queues and modules.
///
/// Also acts as `control` module which handles participant and room states.
pub struct Runner {
    /// Runner ID which is used to assume ownership of a participant id
    runner_id: Uuid,

    /// participant id that the runner is connected to
    id: ParticipantId,

    /// True if a resumption token was used
    resuming: bool,

    /// The database repr of the current room at the time of joining
    room: Room,

    /// Full signaling room id
    room_id: SignalingRoomId,

    /// User behind the participant or Guest
    participant: api::Participant<User>,

    /// The role of the participant inside the room
    role: Role,

    /// The control data. Initialized when frontend send join
    state: RunnerState,

    /// Websocket abstraction which connects the to the websocket actor
    ws: Ws,

    /// All registered and initialized modules
    modules: Modules,
    events: SelectAll<AnyStream>,

    /// Signaling metrics for this runner
    metrics: Arc<SignalingMetrics>,

    /// Database connection pool
    db: Arc<Db>,

    /// Redis connection manager
    redis_conn: RedisConnection,

    /// RabbitMQ queue consumer for this participant, will contain any events about room and
    /// participant changes
    consumer: lapin::Consumer,
    consumer_delegated: bool,

    /// RabbitMQ channel to send events
    rabbitmq_channel: RabbitMqChannel,

    /// Name of the rabbitmq room exchange
    room_exchange: String,

    /// Util to keep the resumption token alive
    resumption_keep_alive: ResumptionTokenKeepAlive,

    /// global application shutdown signal
    shutdown_sig: broadcast::Receiver<()>,

    /// When set to true the runner will gracefully exit on next loop
    exit: bool,

    /// Shared settings of the running program
    settings: SharedSettings,

    time_limit_future: Pin<Box<dyn Future<Output = ()>>>,
}

impl Drop for Runner {
    fn drop(&mut self) {
        // If the runner gets dropped without calling destroy it might have unacknowledged messages
        // in its queue causing the channel to be closed by RabbitMQ.
        //
        // Avoid this by delegating all messages into extra tasks where they will be acknowledged
        if !self.consumer_delegated {
            self.delegate_consumer()
        }
    }
}

/// Current state of the runner
enum RunnerState {
    /// Runner and its rabbitmq resources are created
    /// but has not joined the room yet (no redis resources set)
    None,

    /// Inside the waiting room
    Waiting {
        accepted: bool,
        control_data: ControlData,
    },

    /// Inside the actual room
    Joined,
}

impl Runner {
    #[allow(clippy::too_many_arguments)]
    pub fn builder(
        runner_id: Uuid,
        id: ParticipantId,
        resuming: bool,
        room: Room,
        breakout_room: Option<BreakoutRoomId>,
        participant: api::Participant<User>,
        protocol: &'static str,
        metrics: Arc<SignalingMetrics>,
        db: Arc<Db>,
        storage: Arc<ObjectStorage>,
        authz: Arc<Authz>,
        redis_conn: RedisConnection,
        rabbitmq_channel: RabbitMqChannel,
        resumption_keep_alive: ResumptionTokenKeepAlive,
    ) -> Builder {
        // TODO(r.floren) Change this when the permissions system gets introduced
        let role = match &participant {
            api::Participant::User(user) => {
                if user.id == room.created_by {
                    Role::Moderator
                } else {
                    Role::User
                }
            }
            api::Participant::Guest | api::Participant::Sip | api::Participant::Recorder => {
                Role::Guest
            }
        };

        Builder {
            runner_id,
            id,
            resuming,
            room,
            breakout_room,
            participant,
            role,
            protocol,
            metrics,
            modules: Default::default(),
            rabbitmq_exchanges: vec![],
            rabbitmq_bindings: vec![],
            events: SelectAll::new(),
            db,
            storage,
            authz,
            redis_conn,
            rabbitmq_channel,
            resumption_keep_alive,
        }
    }

    /// Delegate all messages of this consumer to leave no messages unacknowledged until it is properly destroyed
    ///
    /// After calling this function the runner will no longer receive rabbitmq messages
    fn delegate_consumer(&mut self) {
        self.consumer.set_delegate(|delivery: DeliveryResult| {
            Box::pin(async move {
                match delivery {
                    Ok(Some(delivery)) => {
                        if let Err(e) = delivery.acker.nack(Default::default()).await {
                            log::error!("Consumer delegate failed to acknowledge, {}", e);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        log::error!("Delegate received error, {}", e);
                    }
                }
            })
        });

        self.consumer_delegated = true;
    }

    /// Destroys the runner and all associated resources
    #[tracing::instrument(skip(self), fields(id = %self.id))]
    pub async fn destroy(mut self, close_ws: bool) {
        let destroy_start_time = Instant::now();
        let mut encountered_error = false;

        self.delegate_consumer();

        if let RunnerState::Joined | RunnerState::Waiting { .. } = &self.state {
            // The retry/wait_time values are set extra high
            // since a lot of operations are being done while holding the lock
            let mut room_mutex = storage::room_mutex(self.room_id);

            let room_guard = match room_mutex.lock(&mut self.redis_conn).await {
                Ok(guard) => guard,
                Err(r3dlock::Error::Redis(e)) => {
                    log::error!("Failed to acquire r3dlock, {}", e);
                    // There is a problem when accessing redis which could
                    // mean either the network or redis is broken.
                    // Both cases cannot be handled here, abort the cleanup

                    self.metrics
                        .record_destroy_time(destroy_start_time.elapsed().as_secs_f64(), false);

                    return;
                }
                Err(r3dlock::Error::CouldNotAcquireLock) => {
                    log::error!("Failed to acquire r3dlock, contention too high");

                    self.metrics
                        .record_destroy_time(destroy_start_time.elapsed().as_secs_f64(), false);

                    return;
                }
                Err(r3dlock::Error::FailedToUnlock | r3dlock::Error::AlreadyExpired) => {
                    unreachable!()
                }
            };

            if let RunnerState::Joined = &self.state {
                // first check if the list of joined participant is empty
                if let Err(e) = storage::set_attribute(
                    &mut self.redis_conn,
                    self.room_id,
                    self.id,
                    "left_at",
                    Timestamp::now(),
                )
                .await
                {
                    log::error!("failed to mark participant as left, {:?}", e);
                    encountered_error = true;
                }
            } else if let RunnerState::Waiting { .. } = &self.state {
                if let Err(e) = moderation::storage::waiting_room_remove(
                    &mut self.redis_conn,
                    self.room_id.room_id(),
                    self.id,
                )
                .await
                {
                    log::error!(
                        "failed to remove participant from waiting_room list, {:?}",
                        e
                    );
                    encountered_error = true;
                }
                if let Err(e) = moderation::storage::waiting_room_accepted_remove(
                    &mut self.redis_conn,
                    self.room_id.room_id(),
                    self.id,
                )
                .await
                {
                    log::error!(
                        "failed to remove participant from waiting_room_accepted list, {:?}",
                        e
                    );
                    encountered_error = true;
                }
            };

            let room_is_empty =
                match storage::participants_all_left(&mut self.redis_conn, self.room_id).await {
                    Ok(room_is_empty) => room_is_empty,
                    Err(e) => {
                        log::error!("Failed to check if room is empty {:?}", e);
                        encountered_error = true;
                        false
                    }
                };

            // if the room is empty check that the waiting room is empty
            let destroy_room = if room_is_empty {
                if self.room_id.1.is_some() {
                    // Breakout rooms are destroyed even with participants inside the waiting room
                    true
                } else {
                    // destroy room only if waiting room is empty
                    let waiting_room_is_empty = match moderation::storage::waiting_room_len(
                        &mut self.redis_conn,
                        self.room_id.room_id(),
                    )
                    .await
                    {
                        Ok(waiting_room_len) => waiting_room_len == 0,
                        Err(e) => {
                            log::error!("failed to get waiting room len, {:?}", e);
                            encountered_error = true;
                            false
                        }
                    };
                    let waiting_room_accepted_is_empty =
                        match moderation::storage::waiting_room_accepted_len(
                            &mut self.redis_conn,
                            self.room_id.room_id(),
                        )
                        .await
                        {
                            Ok(waiting_room_len) => waiting_room_len == 0,
                            Err(e) => {
                                log::error!("failed to get accepted waiting room len, {:?}", e);
                                encountered_error = true;
                                false
                            }
                        };
                    waiting_room_is_empty && waiting_room_accepted_is_empty
                }
            } else {
                false
            };

            match storage::decrement_participant_count(&mut self.redis_conn, self.room.id).await {
                Ok(remaining_participant_count) => {
                    if remaining_participant_count == 0 {
                        if let Err(e) = self.cleanup_redis_for_global_room().await {
                            log::error!("failed to mark participant as left, {:?}", e);
                            encountered_error = true;
                        }
                    }
                }
                Err(e) => {
                    log::error!("failed to decrement participant count, {:?}", e);
                    encountered_error = true;
                }
            }

            let ctx = DestroyContext {
                redis_conn: &mut self.redis_conn,
                destroy_room,
            };

            self.modules.destroy(ctx).await;

            if destroy_room {
                if let Err(e) = self.cleanup_redis_keys_for_current_room().await {
                    log::error!("Failed to remove all control attributes, {}", e);
                    encountered_error = true;
                }

                self.metrics.increment_destroyed_rooms_count();
            }

            self.metrics.decrement_participants_count(&self.participant);

            if let Err(e) = room_guard.unlock(&mut self.redis_conn).await {
                log::error!("Failed to unlock set_guard r3dlock, {}", e);
                encountered_error = true;
            }

            if !destroy_room {
                match &self.state {
                    RunnerState::None => unreachable!("state was checked before"),
                    RunnerState::Waiting { .. } => {
                        self.rabbitmq_publish(
                            Timestamp::now(),
                            Some(&breakout::rabbitmq::global_exchange_name(
                                self.room_id.room_id(),
                            )),
                            control::rabbitmq::room_all_routing_key(),
                            serde_json::to_string(&NamespacedCommand {
                                namespace: moderation::NAMESPACE,
                                payload: moderation::rabbitmq::Message::LeftWaitingRoom(self.id),
                            })
                            .expect("Failed to convert namespaced to json"),
                        )
                        .await;
                    }
                    RunnerState::Joined => {
                        // Skip sending the left message.
                        // TODO:(kbalt): The left message is the only message not sent by the recorder, all other
                        // messages are currently ignored by filtering in the `build_participant` function
                        // It'd might be nicer to have a "visibility" check before sending any "joined"/"updated"/"left"
                        // message
                        if !matches!(&self.participant, api::Participant::Recorder) {
                            self.rabbitmq_publish_control(
                                Timestamp::now(),
                                None,
                                rabbitmq::Message::Left(self.id),
                            )
                            .await;
                        }
                    }
                }
            }
        } else {
            // Not joined, just destroy modules normal
            let ctx = DestroyContext {
                redis_conn: &mut self.redis_conn,
                destroy_room: false,
            };

            self.modules.destroy(ctx).await;
        }

        // Cancel subscription to not poison the rabbitmq channel with unacknowledged messages
        if let Err(e) = self
            .rabbitmq_channel
            .basic_cancel(self.consumer.tag().as_str(), Default::default())
            .await
        {
            log::error!("Failed to cancel consumer, {}", e);
            encountered_error = true;
        }

        // release participant id
        match redis::cmd("GETDEL")
            .arg(ParticipantIdRunnerLock { id: self.id })
            .query_async::<_, String>(&mut self.redis_conn)
            .await
        {
            Ok(runner_id) => {
                if runner_id != self.runner_id.to_string() {
                    log::warn!("removed runner id does not match the id of the runner");
                }
            }
            Err(e) => {
                log::error!("failed to remove participant id, {}", e);
                encountered_error = true;
            }
        }

        self.metrics.record_destroy_time(
            destroy_start_time.elapsed().as_secs_f64(),
            !encountered_error,
        );

        // If a Close frame is received from the websocket actor, manually return a close command
        if close_ws {
            self.ws.close(CloseCode::Normal).await;
        }
    }

    /// Remove all room and control module related data from redis for the current 'local' room/breakout-room. Does not
    /// touch any keys that contain 'global' data that is used across all 'sub'-rooms (main & breakout rooms).
    async fn cleanup_redis_keys_for_current_room(&mut self) -> Result<()> {
        storage::remove_room_closes_at(&mut self.redis_conn, self.room_id).await?;
        storage::remove_participant_set(&mut self.redis_conn, self.room_id).await?;
        storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "display_name").await?;
        storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "role").await?;
        storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "joined_at").await?;
        storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "left_at").await?;
        storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "hand_is_up").await?;
        storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "hand_updated_at")
            .await?;
        storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "kind").await?;
        storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "user_id").await?;
        storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "avatar_url").await
    }

    /// Remove all room and control module related redis keys that are used across all 'sub'-rooms. This must only be
    /// called once the main and all breakout rooms are empty.
    async fn cleanup_redis_for_global_room(&mut self) -> Result<()> {
        storage::delete_participant_count(&mut self.redis_conn, self.room.id).await?;
        storage::delete_tariff(&mut self.redis_conn, self.room.id).await
    }

    /// Runs the runner until the peer closes its websocket connection or a fatal error occurres.
    pub async fn run(mut self) {
        let mut manual_close_ws = false;

        // Set default `skip_waiting_room` key value with the default expiration time in seconds
        _ = storage::set_skip_waiting_room_with_expiry_nx(
            &mut self.redis_conn,
            self.id,
            false,
            SKIP_WAITING_ROOM_KEY_EXPIRY,
        )
        .await;
        let mut skip_waiting_room_refresh_interval =
            interval(Duration::from_secs(SKIP_WAITING_ROOM_KEY_REFRESH_INTERVAL));

        while matches!(self.ws.state, State::Open) {
            if self.exit && matches!(self.ws.state, State::Open) {
                // This case handles exit on errors unrelated to websocket or controller shutdown
                self.ws.close(CloseCode::Abnormal).await;
            }

            tokio::select! {
                res = self.ws.receive() => {
                    match res {
                        Ok(Some(Message::Close(_))) => {
                            // Received Close frame from ws actor, break to destroy the runner
                            manual_close_ws = true;
                            break;
                        }
                        Ok(Some(msg)) => self.handle_ws_message(msg).await,
                        Ok(None) => {
                            // Ws was in closing state, runner will now exit gracefully
                        }
                        Err(e) => {
                            // Ws is now going to be in error state and cause the runner to exit
                            log::error!("Failed to receive ws message for participant {}, {}", self.id, e);
                        }
                    }
                }
                res = self.consumer.next() => {
                    match res {
                        Some(Ok(delivery)) => self.handle_rabbitmq_msg(delivery).await,
                        _ => {
                            // None or Some(Err(_)), either way its an error to us
                            log::error!("Failed to receive RabbitMQ message, exiting");
                            self.exit = true;
                        }
                    }
                }
                Some((namespace, any)) = self.events.next() => {
                    let timestamp = Timestamp::now();
                    let actions = self.handle_module_targeted_event(namespace, timestamp, DynTargetedEvent::Ext(any))
                        .await
                        .expect("Should not get events from unknown modules");

                    self.handle_module_requested_actions(timestamp, actions).await;
                }
                _ = skip_waiting_room_refresh_interval.tick() => {
                    _ = storage::reset_skip_waiting_room_expiry(
                        &mut self.redis_conn,
                        self.id,
                        SKIP_WAITING_ROOM_KEY_EXPIRY,
                    )
                    .await;
                }
                _ = &mut self.time_limit_future => {
                    self.ws_send_control(Timestamp::now(), outgoing::Message::TimeLimitQuotaElapsed).await;
                    self.ws.close(CloseCode::Normal).await;
                    break;
                }
                _ = self.shutdown_sig.recv() => {
                    self.ws.close(CloseCode::Away).await;
                    break;
                }
                _ = self.resumption_keep_alive.wait() => {
                    if let Err(e) = self.resumption_keep_alive.refresh(&mut self.redis_conn).await {
                        if e.is::<ResumptionTokenUsed>() {
                            log::warn!("Closing connection of this runner as its resumption token was used");

                            self.ws.close(CloseCode::Normal).await;
                        } else {
                            log::error!("failed to set resumption token in redis, {:?}", e);
                        }
                    }
                }
            }
        }
        let timestamp = Timestamp::now();
        let actions = self
            .handle_module_broadcast_event(timestamp, DynBroadcastEvent::Leaving, false)
            .await;

        self.handle_module_requested_actions(timestamp, actions)
            .await;

        log::debug!("Stopping ws-runner task for participant {}", self.id);

        self.destroy(manual_close_ws).await;
    }

    #[tracing::instrument(skip(self, message), fields(id = %self.id))]
    async fn handle_ws_message(&mut self, message: Message) {
        log::trace!("Received websocket message {:?}", message);

        let value: Result<NamespacedCommand<'_, Value>, _> = match message {
            Message::Text(ref text) => serde_json::from_str(text),
            Message::Binary(ref binary) => serde_json::from_slice(binary),
            _ => unreachable!(),
        };

        let timestamp = Timestamp::now();

        let namespaced = match value {
            Ok(value) => value,
            Err(e) => {
                log::error!("Failed to parse namespaced message, {}", e);

                self.ws_send_control_error(timestamp, outgoing::Error::InvalidJson)
                    .await;

                return;
            }
        };

        if namespaced.namespace == NAMESPACE {
            match serde_json::from_value(namespaced.payload) {
                Ok(msg) => {
                    if let Err(e) = self.handle_control_msg(timestamp, msg).await {
                        log::error!("Failed to handle control msg, {}", e);
                        self.exit = true;
                    }
                }
                Err(e) => {
                    log::error!("Failed to parse control payload, {}", e);

                    self.ws_send_control_error(timestamp, outgoing::Error::InvalidJson)
                        .await;
                }
            }
            // Do not handle any other messages than control-join before joined
        } else if let RunnerState::Joined = &self.state {
            match self
                .handle_module_targeted_event(
                    namespaced.namespace,
                    timestamp,
                    DynTargetedEvent::WsMessage(namespaced.payload),
                )
                .await
            {
                Ok(actions) => {
                    self.handle_module_requested_actions(timestamp, actions)
                        .await
                }
                Err(NoSuchModuleError(())) => {
                    self.ws_send_control_error(timestamp, outgoing::Error::InvalidNamespace)
                        .await;
                }
            }
        }
    }

    async fn handle_control_msg(
        &mut self,
        timestamp: Timestamp,
        msg: incoming::Message,
    ) -> Result<()> {
        match msg {
            incoming::Message::Join(join) => {
                if !matches!(self.state, RunnerState::None) {
                    self.ws_send_control_error(timestamp, outgoing::Error::AlreadyJoined)
                        .await;

                    return Ok(());
                }

                let (display_name, avatar_url) = match &self.participant {
                    api::Participant::User(user) => {
                        let avatar_url = Some(format!(
                            "{}{:x}",
                            self.settings.load().avatar.libravatar_url,
                            md5::compute(&user.email)
                        ));

                        (trim_display_name(join.display_name), avatar_url)
                    }
                    api::Participant::Guest => (trim_display_name(join.display_name), None),
                    api::Participant::Recorder => (join.display_name, None),
                    api::Participant::Sip => {
                        if let Some(call_in) = self.settings.load().call_in.as_ref() {
                            let display_name = sip::display_name(
                                &self.db,
                                call_in,
                                self.room.tenant_id,
                                join.display_name,
                            )
                            .await;

                            (display_name, None)
                        } else {
                            (trim_display_name(join.display_name), None)
                        }
                    }
                };

                if display_name.is_empty() || display_name.len() > 100 {
                    self.ws_send_control_error(timestamp, outgoing::Error::InvalidUsername)
                        .await;
                }

                self.set_control_attributes(timestamp, &display_name, avatar_url.as_deref())
                    .await?;

                let control_data = ControlData {
                    display_name,
                    role: self.role,
                    avatar_url,
                    participation_kind: match &self.participant {
                        api::Participant::User(_) => ParticipationKind::User,
                        api::Participant::Guest => ParticipationKind::Guest,
                        api::Participant::Sip => ParticipationKind::Sip,
                        api::Participant::Recorder => ParticipationKind::Recorder,
                    },
                    joined_at: timestamp,
                    hand_is_up: false,
                    hand_updated_at: timestamp,
                    left_at: None,
                };

                self.metrics.increment_participants_count(&self.participant);

                // Allow moderators, invisible services, and already accepted participants to skip the waiting room
                let can_skip_waiting_room: bool =
                    storage::get_skip_waiting_room(&mut self.redis_conn, self.id).await?;

                let skip_waiting_room = matches!(self.role, Role::Moderator)
                    || !control_data.participation_kind.is_visible()
                    || can_skip_waiting_room;

                let waiting_room_enabled = moderation::storage::init_waiting_room_key(
                    &mut self.redis_conn,
                    self.room_id.room_id(),
                    self.room.waiting_room,
                )
                .await?;

                if !skip_waiting_room && waiting_room_enabled {
                    // Waiting room is enabled; join the waiting room
                    self.join_waiting_room(timestamp, control_data).await?;
                } else {
                    // Waiting room is not enabled; join the room directly
                    self.join_room(timestamp, control_data, false).await?;
                }
            }
            incoming::Message::EnterRoom => {
                match replace(&mut self.state, RunnerState::None) {
                    RunnerState::Waiting {
                        accepted: true,
                        control_data,
                    } => {
                        self.rabbitmq_publish(
                            timestamp,
                            Some(
                                breakout::rabbitmq::global_exchange_name(self.room_id.room_id())
                                    .as_str(),
                            ),
                            control::rabbitmq::room_all_routing_key(),
                            serde_json::to_string(&NamespacedCommand {
                                namespace: moderation::NAMESPACE,
                                payload: moderation::rabbitmq::Message::LeftWaitingRoom(self.id),
                            })
                            .expect("Failed to convert namespaced to json"),
                        )
                        .await;

                        moderation::storage::waiting_room_accepted_remove(
                            &mut self.redis_conn,
                            self.room_id.room_id(),
                            self.id,
                        )
                        .await?;

                        self.join_room(timestamp, control_data, true).await?
                    }
                    // not in correct state, reset it
                    state => {
                        self.state = state;

                        self.ws_send_control_error(
                            timestamp,
                            outgoing::Error::NotAcceptedOrNotInWaitingRoom,
                        )
                        .await;
                    }
                }
            }
            incoming::Message::RaiseHand => {
                if !moderation::storage::is_raise_hands_enabled(&mut self.redis_conn, self.room.id)
                    .await?
                {
                    self.ws_send_control_error(timestamp, outgoing::Error::RaiseHandsDisabled)
                        .await;

                    return Ok(());
                }

                self.handle_raise_hand_change(timestamp, true).await?;
            }
            incoming::Message::LowerHand => {
                self.handle_raise_hand_change(timestamp, false).await?;
            }
            incoming::Message::GrantModeratorRole(incoming::Target { target }) => {
                if !matches!(self.state, RunnerState::Joined) {
                    self.ws_send_control_error(timestamp, outgoing::Error::NotYetJoined)
                        .await;

                    return Ok(());
                }

                self.handle_grant_moderator_msg(timestamp, target, true)
                    .await?;
            }
            incoming::Message::RevokeModeratorRole(incoming::Target { target }) => {
                if !matches!(self.state, RunnerState::Joined) {
                    self.ws_send_control_error(timestamp, outgoing::Error::NotYetJoined)
                        .await;

                    return Ok(());
                }

                self.handle_grant_moderator_msg(timestamp, target, false)
                    .await?;
            }
        }

        Ok(())
    }

    async fn handle_grant_moderator_msg(
        &mut self,
        timestamp: Timestamp,
        target: ParticipantId,
        grant: bool,
    ) -> Result<()> {
        if self.role != Role::Moderator {
            self.ws_send_control_error(timestamp, outgoing::Error::InsufficientPermissions)
                .await;

            return Ok(());
        }

        let role: Option<Role> =
            storage::get_attribute(&mut self.redis_conn, self.room_id, target, "role").await?;

        let is_moderator = matches!(role, Some(Role::Moderator));

        if is_moderator == grant {
            self.ws_send_control_error(timestamp, outgoing::Error::NothingToDo)
                .await;

            return Ok(());
        }

        let user_id: Option<UserId> =
            storage::get_attribute(&mut self.redis_conn, self.room_id, target, "user_id").await?;

        if let Some(user_id) = user_id {
            if user_id == self.room.created_by {
                self.ws_send_control_error(timestamp, outgoing::Error::TargetIsRoomOwner)
                    .await;

                return Ok(());
            }
        }

        self.rabbitmq_publish_control(
            timestamp,
            Some(target),
            rabbitmq::Message::SetModeratorStatus(grant),
        )
        .await;

        Ok(())
    }

    async fn handle_raise_hand_change(
        &mut self,
        timestamp: Timestamp,
        hand_raised: bool,
    ) -> Result<()> {
        storage::AttrPipeline::new(self.room_id, self.id)
            .set("hand_is_up", hand_raised)
            .set("hand_updated_at", timestamp)
            .query_async(&mut self.redis_conn)
            .await?;

        let broadcast_event = if hand_raised {
            DynBroadcastEvent::RaiseHand
        } else {
            DynBroadcastEvent::LowerHand
        };
        let actions = self
            .handle_module_broadcast_event(timestamp, broadcast_event, true)
            .await;

        self.handle_module_requested_actions(timestamp, actions)
            .await;

        Ok(())
    }

    /// Enforces the given tariff.
    ///
    /// Requires the room lock to be taken before calling
    async fn enforce_tariff(
        &mut self,
        tariff: Tariff,
    ) -> Result<ControlFlow<JoinBlockedReason, Tariff>> {
        let tariff =
            control::storage::try_init_tariff(&mut self.redis_conn, self.room.id, tariff).await?;

        if let Some(participant_limit) = tariff.quotas.0.get("room_participant_limit") {
            if let Some(count) =
                control::storage::get_participant_count(&mut self.redis_conn, self.room.id).await?
            {
                if count >= *participant_limit as isize {
                    return Ok(ControlFlow::Break(
                        JoinBlockedReason::ParticipantLimitReached,
                    ));
                }
            }
        }

        control::storage::increment_participant_count(&mut self.redis_conn, self.room.id).await?;

        Ok(ControlFlow::Continue(tariff))
    }

    async fn join_waiting_room(
        &mut self,
        timestamp: Timestamp,
        control_data: ControlData,
    ) -> Result<()> {
        let db = self.db.clone();
        let creator_id = self.room.created_by;

        let tariff = crate::block(move || Tariff::get_by_user_id(&mut db.get_conn()?, &creator_id))
            .await??;

        let mut lock = storage::room_mutex(self.room_id);
        let guard = lock.lock(&mut self.redis_conn).await?;

        match self.enforce_tariff(tariff).await {
            Ok(ControlFlow::Continue(_)) => { /* continue */ }
            Ok(ControlFlow::Break(reason)) => {
                guard.unlock(&mut self.redis_conn).await?;

                self.ws_send_control(Timestamp::now(), outgoing::Message::JoinBlocked(reason))
                    .await;

                return Ok(());
            }
            Err(e) => {
                guard.unlock(&mut self.redis_conn).await?;

                return Err(e);
            }
        };

        let res = moderation::storage::waiting_room_add(
            &mut self.redis_conn,
            self.room_id.room_id(),
            self.id,
        )
        .await;

        guard.unlock(&mut self.redis_conn).await?;
        let num_added = res?;

        // Check that SADD doesn't return 0. That would mean that the participant id would be a
        // duplicate which cannot be allowed. Since this should never happen just error and exit.
        if !self.resuming && num_added == 0 {
            bail!("participant-id is already taken inside waiting-room set");
        }

        self.state = RunnerState::Waiting {
            accepted: false,
            control_data,
        };

        self.ws
            .send(Message::Text(
                serde_json::to_string(&NamespacedEvent {
                    namespace: moderation::NAMESPACE,
                    timestamp,
                    payload: moderation::outgoing::Message::InWaitingRoom,
                })?
                .into(),
            ))
            .await;

        self.rabbitmq_publish(
            timestamp,
            Some(breakout::rabbitmq::global_exchange_name(self.room_id.room_id()).as_str()),
            control::rabbitmq::room_all_routing_key(),
            serde_json::to_string(&NamespacedCommand {
                namespace: moderation::NAMESPACE,
                payload: moderation::rabbitmq::Message::JoinedWaitingRoom(self.id),
            })
            .expect("Failed to convert namespaced to json"),
        )
        .await;

        Ok(())
    }

    async fn join_room(
        &mut self,
        timestamp: Timestamp,
        control_data: ControlData,
        joining_from_waiting_room: bool,
    ) -> Result<()> {
        let mut lock = storage::room_mutex(self.room_id);

        // If we haven't joined the waiting room yet, fetch, set and enforce the tariff for the room.
        // When in waiting-room this logic was already executed in `join_waiting_room`.
        let (guard, tariff) = if !joining_from_waiting_room {
            let db = self.db.clone();
            let creator_id = self.room.created_by;

            let mut tariff =
                crate::block(move || Tariff::get_by_user_id(&mut db.get_conn()?, &creator_id))
                    .await??;

            let guard = lock.lock(&mut self.redis_conn).await?;

            match self.enforce_tariff(tariff.clone()).await {
                Ok(ControlFlow::Continue(enforced_tariff)) => {
                    tariff = enforced_tariff;
                }
                Ok(ControlFlow::Break(reason)) => {
                    guard.unlock(&mut self.redis_conn).await?;

                    self.ws_send_control(Timestamp::now(), outgoing::Message::JoinBlocked(reason))
                        .await;

                    return Ok(());
                }
                Err(e) => {
                    guard.unlock(&mut self.redis_conn).await?;

                    return Err(e);
                }
            };

            (guard, tariff)
        } else {
            let tariff = control::storage::get_tariff(&mut self.redis_conn, self.room.id).await?;
            (lock.lock(&mut self.redis_conn).await?, tariff)
        };

        let res = self.join_room_locked().await;

        let unlock_res = guard.unlock(&mut self.redis_conn).await;

        let participant_ids = match res {
            Ok(participants) => participants,
            Err(e) => {
                bail!("Failed to join room, {e:?}\nUnlocked room lock, {unlock_res:?}");
            }
        };

        unlock_res?;

        let mut participants = vec![];

        for id in participant_ids {
            if self.id == id {
                continue;
            }

            match self.build_participant(id).await {
                Ok(Some(participant)) => participants.push(participant),
                Ok(None) => { /* ignore invisible participants */ }
                Err(e) => log::error!("Failed to build participant {}, {}", id, e),
            };
        }

        let mut module_data = HashMap::new();

        let actions = self
            .handle_module_broadcast_event(
                timestamp,
                DynBroadcastEvent::Joined(&control_data, &mut module_data, &mut participants),
                false,
            )
            .await;

        let available_modules = self.modules.get_module_names();
        let closes_at =
            control::storage::get_room_closes_at(&mut self.redis_conn, self.room_id).await?;

        self.ws_send_control(
            timestamp,
            outgoing::Message::JoinSuccess(outgoing::JoinSuccess {
                id: self.id,
                display_name: control_data.display_name.clone(),
                avatar_url: control_data.avatar_url.clone(),
                role: self.role,
                closes_at,
                tariff: TariffResource::from_tariff(tariff, &available_modules).into(),
                module_data,
                participants,
            }),
        )
        .await;

        self.state = RunnerState::Joined;

        self.rabbitmq_publish_control(timestamp, None, rabbitmq::Message::Joined(self.id))
            .await;

        self.handle_module_requested_actions(timestamp, actions)
            .await;

        Ok(())
    }

    async fn join_room_locked(&mut self) -> Result<Vec<ParticipantId>> {
        let participant_set_exists =
            control::storage::participant_set_exists(&mut self.redis_conn, self.room_id).await?;

        if !participant_set_exists {
            self.set_room_time_limit().await?;
            self.metrics.increment_created_rooms_count();
        }
        self.activate_room_time_limit().await?;

        let participants = storage::get_all_participants(&mut self.redis_conn, self.room_id)
            .await
            .context("Failed to get all active participants")?;

        let num_added =
            storage::add_participant_to_set(&mut self.redis_conn, self.room_id, self.id)
                .await
                .context("Failed to add self to participants set")?;

        // Check that SADD doesn't return 0. That would mean that the participant id would be a
        // duplicate which cannot be allowed. Since this should never happen just error and exit.
        if !self.resuming && num_added == 0 {
            bail!("participant-id is already taken inside participant set");
        }

        Ok(participants)
    }

    async fn set_room_time_limit(&mut self) -> Result<(), anyhow::Error> {
        let tariff = storage::get_tariff(&mut self.redis_conn, self.room.id).await?;

        let quotas = tariff.quotas.0;
        let remaining_seconds = quotas
            .get("room_time_limit_secs")
            .map(|time_limit| *time_limit as i64);

        if let Some(remaining_seconds) = remaining_seconds {
            let closes_at =
                Timestamp::now().checked_add_signed(chrono::Duration::seconds(remaining_seconds));

            if let Some(closes_at) = closes_at {
                control::storage::set_room_closes_at(
                    &mut self.redis_conn,
                    self.room_id,
                    closes_at.into(),
                )
                .await?;
            } else {
                log::error!("DateTime overflow for closes_at");
            }
        }

        Ok(())
    }

    async fn activate_room_time_limit(&mut self) -> Result<(), anyhow::Error> {
        let closes_at =
            control::storage::get_room_closes_at(&mut self.redis_conn, self.room_id).await?;

        if let Some(closes_at) = closes_at {
            let remaining_seconds = (*closes_at - *Timestamp::now()).num_seconds();
            let future = tokio::time::sleep(Duration::from_secs(remaining_seconds.max(0) as u64));
            self.time_limit_future = Box::pin(future);
        }

        Ok(())
    }

    async fn set_control_attributes(
        &mut self,
        timestamp: Timestamp,
        display_name: &str,
        avatar_url: Option<&str>,
    ) -> Result<()> {
        let mut pipe_attrs = storage::AttrPipeline::new(self.room_id, self.id);

        match &self.participant {
            api::Participant::User(ref user) => {
                pipe_attrs
                    .set("kind", ParticipationKind::User)
                    .set(
                        "avatar_url",
                        avatar_url.expect("user must have avatar_url set"),
                    )
                    .set("user_id", user.id);
            }
            api::Participant::Guest => {
                pipe_attrs.set("kind", ParticipationKind::Guest);
            }
            api::Participant::Sip => {
                pipe_attrs.set("kind", ParticipationKind::Sip);
            }
            api::Participant::Recorder => {
                pipe_attrs.set("kind", ParticipationKind::Recorder);
            }
        }

        pipe_attrs
            .set("role", self.role)
            .set("hand_is_up", false)
            .set("hand_updated_at", timestamp)
            .set("display_name", display_name)
            .set("joined_at", timestamp)
            .del("left_at")
            .query_async(&mut self.redis_conn)
            .await?;

        Ok(())
    }

    /// Fetch all control related data for the given participant id, building a "base" for a participant.
    ///
    /// If the participant is an invisible service (like the recorder) and shouldn't be shown to other participants
    /// this function will return Ok(None)
    async fn build_participant(&mut self, id: ParticipantId) -> Result<Option<Participant>> {
        let mut participant = outgoing::Participant {
            id,
            module_data: Default::default(),
        };

        let control_data = ControlData::from_redis(&mut self.redis_conn, self.room_id, id).await?;

        // Do not build participants for invisible services
        if !control_data.participation_kind.is_visible() {
            return Ok(None);
        };

        participant.module_data.insert(
            NAMESPACE,
            serde_json::to_value(control_data)
                .expect("Failed to convert ControlData to serde_json::Value"),
        );

        Ok(Some(participant))
    }

    #[tracing::instrument(skip(self, delivery), fields(id = %self.id))]
    async fn handle_rabbitmq_msg(&mut self, delivery: lapin::message::Delivery) {
        if let Err(e) = delivery.acker.ack(Default::default()).await {
            log::warn!("Failed to ACK incoming delivery, {}", e);
        }

        // RabbitMQ messages can come from a variety of places
        // First check if it is a signaling message,
        // then if it is a ha_sync::user_update message
        if delivery.exchange.as_str().starts_with("k3k-signaling") {
            // Do not handle any messages before the user joined the room
            if let RunnerState::None = &self.state {
                return;
            }

            let timestamp: Timestamp = delivery.properties.timestamp().map(|value| chrono::Utc.timestamp_opt(value as i64, 0).latest()).flatten().unwrap_or_else(|| {
                log::warn!("Got RabbitMQ message without timestamp. Creating current timestamp as fallback.");
                chrono::Utc::now()
            }
            ).into();

            let namespaced =
                match serde_json::from_slice::<NamespacedCommand<Value>>(&delivery.data) {
                    Ok(namespaced) => namespaced,
                    Err(e) => {
                        log::error!("Failed to read incoming rabbit-mq message, {}", e);
                        return;
                    }
                };

            if namespaced.namespace == NAMESPACE {
                let msg = match serde_json::from_value::<rabbitmq::Message>(namespaced.payload) {
                    Ok(msg) => msg,
                    Err(e) => {
                        log::error!("Failed to read incoming control rabbit-mq message, {}", e);
                        return;
                    }
                };

                if let Err(e) = self.handle_rabbitmq_control_msg(timestamp, msg).await {
                    log::error!("Failed to handle incoming rabbitmq control msg, {}", e);
                }
            } else if let RunnerState::Joined = &self.state {
                // Only allow rmq messages outside the control namespace if the participant is fully joined
                match self
                    .handle_module_targeted_event(
                        namespaced.namespace,
                        timestamp,
                        DynTargetedEvent::RabbitMqMessage(namespaced.payload),
                    )
                    .await
                {
                    Ok(actions) => {
                        self.handle_module_requested_actions(timestamp, actions)
                            .await
                    }
                    Err(NoSuchModuleError(())) => log::warn!("Got invalid rabbit-mq message"),
                }
            }
        } else {
            log::error!(
                "Unknown message received, routing_key={:?}",
                delivery.routing_key
            );
        }
    }

    async fn handle_rabbitmq_control_msg(
        &mut self,
        timestamp: Timestamp,
        msg: rabbitmq::Message,
    ) -> Result<()> {
        log::debug!("Received RabbitMQ control message {:?}", msg);

        match msg {
            rabbitmq::Message::Joined(id) => {
                // Ignore events of self and only if runner is joined
                if self.id == id || !matches!(&self.state, RunnerState::Joined) {
                    return Ok(());
                }

                let mut participant = if let Some(participant) = self.build_participant(id).await? {
                    participant
                } else {
                    return Ok(());
                };

                let actions = self
                    .handle_module_broadcast_event(
                        timestamp,
                        DynBroadcastEvent::ParticipantJoined(&mut participant),
                        false,
                    )
                    .await;

                self.ws_send_control(timestamp, outgoing::Message::Joined(participant))
                    .await;

                self.handle_module_requested_actions(timestamp, actions)
                    .await;
            }
            rabbitmq::Message::Left(id) => {
                // Ignore events of self and only if runner is joined
                if self.id == id || !matches!(&self.state, RunnerState::Joined) {
                    return Ok(());
                }

                let actions = self
                    .handle_module_broadcast_event(
                        timestamp,
                        DynBroadcastEvent::ParticipantLeft(id),
                        false,
                    )
                    .await;

                self.ws_send_control(
                    timestamp,
                    outgoing::Message::Left(outgoing::AssociatedParticipant { id }),
                )
                .await;

                self.handle_module_requested_actions(timestamp, actions)
                    .await;
            }
            rabbitmq::Message::Update(id) => {
                // Ignore updates of self and only if runner is joined
                if self.id == id || !matches!(&self.state, RunnerState::Joined) {
                    return Ok(());
                }

                let mut participant = if let Some(participant) = self.build_participant(id).await? {
                    participant
                } else {
                    log::warn!("ignoring update of invisible participant");
                    return Ok(());
                };

                let actions = self
                    .handle_module_broadcast_event(
                        timestamp,
                        DynBroadcastEvent::ParticipantUpdated(&mut participant),
                        false,
                    )
                    .await;

                self.ws_send_control(timestamp, outgoing::Message::Update(participant))
                    .await;

                self.handle_module_requested_actions(timestamp, actions)
                    .await;
            }
            rabbitmq::Message::Accepted(id) => {
                if self.id != id {
                    log::warn!("Received misrouted control#accepted message");
                    return Ok(());
                }

                if let RunnerState::Waiting {
                    accepted,
                    control_data: _,
                } = &mut self.state
                {
                    if !*accepted {
                        *accepted = true;

                        // Allow the participant to skip future waiting room once they are accepted
                        // Set the key with the given expiration time in seconds
                        storage::set_skip_waiting_room_with_expiry(
                            &mut self.redis_conn,
                            self.id,
                            true,
                            SKIP_WAITING_ROOM_KEY_EXPIRY,
                        )
                        .await?;

                        self.ws
                            .send(Message::Text(
                                serde_json::to_string(&NamespacedEvent {
                                    namespace: moderation::NAMESPACE,
                                    timestamp,
                                    payload: moderation::outgoing::Message::Accepted,
                                })?
                                .into(),
                            ))
                            .await;
                    }
                }
            }
            rabbitmq::Message::SetModeratorStatus(grant_moderator) => {
                let created_room = if let api::Participant::User(user) = &self.participant {
                    self.room.created_by == user.id
                } else {
                    false
                };

                if created_room {
                    return Ok(());
                }

                let new_role = if grant_moderator {
                    Role::Moderator
                } else {
                    match &self.participant {
                        api::Participant::User(_) => Role::User,
                        api::Participant::Guest
                        | api::Participant::Sip
                        | api::Participant::Recorder => Role::Guest,
                    }
                };

                if self.role == new_role {
                    return Ok(());
                }

                self.role = new_role;

                storage::set_attribute(
                    &mut self.redis_conn,
                    self.room_id,
                    self.id,
                    "role",
                    new_role,
                )
                .await?;

                self.ws_send_control(timestamp, outgoing::Message::RoleUpdated { new_role })
                    .await;

                self.rabbitmq_publish_control(timestamp, None, rabbitmq::Message::Update(self.id))
                    .await;
            }
            rabbitmq::Message::ResetRaisedHands { issued_by } => {
                let raised: Option<bool> = storage::get_attribute(
                    &mut self.redis_conn,
                    self.room_id,
                    self.id,
                    "hand_is_up",
                )
                .await?;
                if matches!(raised, Some(true)) {
                    self.handle_raise_hand_change(timestamp, false).await?;
                }

                self.ws
                    .send(Message::Text(
                        serde_json::to_string(&NamespacedEvent {
                            namespace: moderation::NAMESPACE,
                            timestamp,
                            payload: moderation::outgoing::Message::RaisedHandResetByModerator {
                                issued_by,
                            },
                        })?
                        .into(),
                    ))
                    .await;
            }
            rabbitmq::Message::EnableRaiseHands { issued_by } => {
                self.ws
                    .send(Message::Text(
                        serde_json::to_string(&NamespacedEvent {
                            namespace: moderation::NAMESPACE,
                            timestamp,
                            payload: moderation::outgoing::Message::RaiseHandsEnabled { issued_by },
                        })?
                        .into(),
                    ))
                    .await;
            }
            rabbitmq::Message::DisableRaiseHands { issued_by } => {
                let raised: Option<bool> = storage::get_attribute(
                    &mut self.redis_conn,
                    self.room_id,
                    self.id,
                    "hand_is_up",
                )
                .await?;
                if matches!(raised, Some(true)) {
                    self.handle_raise_hand_change(timestamp, false).await?;
                }

                self.ws
                    .send(Message::Text(
                        serde_json::to_string(&NamespacedEvent {
                            namespace: moderation::NAMESPACE,
                            timestamp,
                            payload: moderation::outgoing::Message::RaiseHandsDisabled {
                                issued_by,
                            },
                        })?
                        .into(),
                    ))
                    .await;
            }
        }

        Ok(())
    }

    /// Send a control message via rabbitmq
    ///
    /// If recipient is `None` the message is sent to all inside the room
    async fn rabbitmq_publish_control(
        &mut self,
        timestamp: Timestamp,
        recipient: Option<ParticipantId>,
        message: rabbitmq::Message,
    ) {
        let message = NamespacedCommand {
            namespace: NAMESPACE,
            payload: message,
        };

        let routing_key = if let Some(recipient) = recipient {
            Cow::Owned(rabbitmq::room_participant_routing_key(recipient))
        } else {
            Cow::Borrowed(rabbitmq::room_all_routing_key())
        };

        self.rabbitmq_publish(
            timestamp,
            None,
            &routing_key,
            serde_json::to_string(&message).expect("Failed to convert namespaced to json"),
        )
        .await;
    }

    /// Publish a rabbitmq message
    ///
    /// If exchange is `None`, `self.room_exchange` will be used.
    #[tracing::instrument(
        skip(self, exchange, message),
        fields(id = %self.id, exchange = %exchange.unwrap_or(&self.room_exchange))
    )]
    async fn rabbitmq_publish(
        &mut self,
        timestamp: Timestamp,
        exchange: Option<&str>,
        routing_key: &str,
        message: String,
    ) {
        log::trace!("publish {}", message);
        let properties = BasicProperties::default().with_timestamp(timestamp.timestamp() as u64);
        if let Err(e) = self
            .rabbitmq_channel
            .basic_publish(
                exchange.unwrap_or(&self.room_exchange),
                routing_key,
                Default::default(),
                message.as_bytes(),
                properties,
            )
            .await
        {
            log::error!("Failed to send message over rabbitmq, {}", e);
            self.exit = true;
        }
    }

    /// Dispatch owned event to a single module
    async fn handle_module_targeted_event(
        &mut self,
        module: &str,
        timestamp: Timestamp,
        dyn_event: DynTargetedEvent,
    ) -> Result<ModuleRequestedActions, NoSuchModuleError> {
        let mut ws_messages = vec![];
        let mut rabbitmq_publish = vec![];
        let mut invalidate_data = false;
        let mut exit = None;

        let ctx = DynEventCtx {
            id: self.id,
            role: self.role,
            timestamp,
            ws_messages: &mut ws_messages,
            rabbitmq_publish: &mut rabbitmq_publish,
            redis_conn: &mut self.redis_conn,
            events: &mut self.events,
            invalidate_data: &mut invalidate_data,
            exit: &mut exit,
            metrics: self.metrics.clone(),
        };

        self.modules
            .on_event_targeted(ctx, module, dyn_event)
            .await?;

        Ok(ModuleRequestedActions {
            ws_messages,
            rabbitmq_publish,
            invalidate_data,
            exit,
        })
    }

    /// Dispatch copyable event to all modules
    async fn handle_module_broadcast_event(
        &mut self,
        timestamp: Timestamp,
        dyn_event: DynBroadcastEvent<'_>,
        mut invalidate_data: bool,
    ) -> ModuleRequestedActions {
        let mut ws_messages = vec![];
        let mut rabbitmq_publish = vec![];
        let mut exit = None;

        let ctx = DynEventCtx {
            id: self.id,
            role: self.role,
            timestamp,
            ws_messages: &mut ws_messages,
            rabbitmq_publish: &mut rabbitmq_publish,
            redis_conn: &mut self.redis_conn,
            events: &mut self.events,
            invalidate_data: &mut invalidate_data,
            exit: &mut exit,
            metrics: self.metrics.clone(),
        };

        self.modules.on_event_broadcast(ctx, dyn_event).await;

        ModuleRequestedActions {
            ws_messages,
            rabbitmq_publish,
            invalidate_data,
            exit,
        }
    }

    /// Modules can request certain actions via the module context (e.g send websocket msg)
    /// these are executed here
    async fn handle_module_requested_actions(
        &mut self,
        timestamp: Timestamp,
        ModuleRequestedActions {
            ws_messages,
            rabbitmq_publish,
            invalidate_data,
            exit,
        }: ModuleRequestedActions,
    ) {
        for ws_message in ws_messages {
            self.ws.send(ws_message).await;
        }

        for publish in rabbitmq_publish {
            self.rabbitmq_publish(
                timestamp,
                publish.exchange.as_deref(),
                &publish.routing_key,
                publish.message,
            )
            .await;
        }

        if invalidate_data {
            self.rabbitmq_publish_control(timestamp, None, rabbitmq::Message::Update(self.id))
                .await;
        }

        if let Some(exit) = exit {
            self.exit = true;

            self.ws.close(exit).await;
        }
    }

    async fn ws_send_control_error(&mut self, timestamp: Timestamp, error: outgoing::Error) {
        self.ws_send_control(timestamp, outgoing::Message::Error(error))
            .await;
    }

    async fn ws_send_control(&mut self, timestamp: Timestamp, payload: outgoing::Message) {
        self.ws
            .send(Message::Text(
                serde_json::to_string(&NamespacedEvent {
                    namespace: NAMESPACE,
                    timestamp,
                    payload,
                })
                .expect("Failed to convert namespaced to json")
                .into(),
            ))
            .await;
    }
}

#[must_use]
struct ModuleRequestedActions {
    ws_messages: Vec<Message>,
    rabbitmq_publish: Vec<RabbitMqPublish>,
    invalidate_data: bool,
    exit: Option<CloseCode>,
}

struct Ws {
    to_actor: Addr<WebSocketActor>,
    from_actor: mpsc::UnboundedReceiver<ws::Message>,

    state: State,
}

enum State {
    Open,
    Closed,
    Error,
}

impl Ws {
    /// Send message via websocket
    async fn send(&mut self, message: Message) {
        if let State::Open = self.state {
            log::trace!("Send message to websocket: {:?}", message);

            if let Err(e) = self.to_actor.send(WsCommand::Ws(message)).await {
                log::error!("Failed to send websocket message, {}", e);
                self.state = State::Error;
            }
        } else {
            log::warn!("Tried to send websocket message on closed or error'd websocket");
        }
    }

    /// Close the websocket connection if needed
    async fn close(&mut self, code: CloseCode) {
        if !matches!(self.state, State::Open) {
            return;
        }

        let reason = CloseReason {
            code,
            description: None,
        };

        log::debug!("closing websocket with code {:?}", code);

        if let Err(e) = self.to_actor.send(WsCommand::Close(reason)).await {
            log::error!("Failed to close websocket, {}", e);
            self.state = State::Error;
        }
    }

    /// Receive a message from the websocket
    ///
    /// Sends a health check ping message every WS_TIMEOUT.
    async fn receive(&mut self) -> Result<Option<Message>> {
        match self.from_actor.recv().await {
            Some(msg) => Ok(Some(msg)),
            None => {
                self.state = State::Closed;
                Ok(None)
            }
        }
    }
}

/// Trim leading, trailing, and extra whitespaces between a given display name.
fn trim_display_name(display_name: String) -> String {
    display_name.split_whitespace().join(" ")
}

#[cfg(test)]
mod test {
    use super::trim_display_name;
    use pretty_assertions::assert_eq;

    #[test]
    fn trim_display_name_leading_spaces() {
        assert_eq!("First Last", trim_display_name("  First Last".to_string()));
    }

    #[test]
    fn trim_display_name_trailing_spaces() {
        assert_eq!("First Last", trim_display_name("First Last  ".to_string()));
    }

    #[test]
    fn trim_display_name_spaces_between() {
        assert_eq!("First Last", trim_display_name("First  Last".to_string()));
    }
}
