use super::actor::WebSocketActor;
use super::modules::{
    AnyStream, DynBroadcastEvent, DynEventCtx, DynTargetedEvent, Modules, NoSuchModuleError,
};
use super::{
    DestroyContext, Namespaced, NamespacedOutgoing, RabbitMqBinding, RabbitMqExchange,
    RabbitMqPublish, Timestamp,
};
use crate::api;
use crate::api::signaling::prelude::*;
use crate::api::signaling::resumption::{ResumptionTokenKeepAlive, ResumptionTokenUsed};
use crate::api::signaling::ws::actor::WsCommand;
use crate::api::signaling::ws_modules::breakout::BreakoutRoomId;
use crate::api::signaling::ws_modules::control::outgoing::Participant;
use crate::api::signaling::ws_modules::control::storage::ParticipantIdRunnerLock;
use crate::api::signaling::ws_modules::control::{
    incoming, outgoing, rabbitmq, storage, ControlData, ParticipationKind, NAMESPACE,
};
use crate::api::signaling::{Role, SignalingRoomId};
use crate::ha_sync::user_update;
use actix::Addr;
use actix_http::ws::{CloseCode, CloseReason, Message};
use actix_web_actors::ws;
use anyhow::{bail, Context, Result};
use bytestring::ByteString;
use chrono::TimeZone;
use controller_shared::settings::SharedSettings;
use controller_shared::ParticipantId;
use database::Db;
use db_storage::rooms::Room;
use db_storage::users::User;
use futures::stream::SelectAll;
use kustos::Authz;
use lapin::message::DeliveryResult;
use lapin::options::{ExchangeDeclareOptions, QueueDeclareOptions};
use lapin::{BasicProperties, ExchangeKind};
use redis::aio::ConnectionManager;
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio::time::sleep;
use tokio_stream::StreamExt;
use uuid::Uuid;

/// Builder to the runner type.
///
/// Passed into [`ModuleBuilder::build`](super::modules::ModuleBuilder::build) function to create an [`InitContext`](super::InitContext).
pub struct Builder {
    runner_id: Uuid,
    pub(super) id: ParticipantId,
    pub(super) room: Room,
    pub(super) breakout_room: Option<BreakoutRoomId>,
    pub(super) participant: api::Participant<User>,
    pub(super) role: Role,
    pub(super) protocol: &'static str,
    pub(super) modules: Modules,
    pub(super) rabbitmq_exchanges: Vec<RabbitMqExchange>,
    pub(super) rabbitmq_bindings: Vec<RabbitMqBinding>,
    pub(super) events: SelectAll<AnyStream>,
    pub(super) db: Arc<Db>,
    pub(super) authz: Arc<Authz>,
    pub(super) redis_conn: ConnectionManager,
    pub(super) rabbitmq_channel: lapin::Channel,
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
                    user_update::EXCHANGE,
                    &user_update::routing_key(user.id),
                    Default::default(),
                    Default::default(),
                )
                .await?;

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
            room_id,
            participant: self.participant,
            role: self.role,
            control_data: None,
            ws: Ws {
                to_actor: to_ws_actor,
                from_actor: from_ws_actor,
                state: State::Open,
            },
            modules: self.modules,
            events: self.events,
            redis_conn: self.redis_conn,
            consumer,
            consumer_delegated: false,
            rabbit_mq_channel: self.rabbitmq_channel,
            room_exchange,
            resumption_keep_alive: self.resumption_keep_alive,
            shutdown_sig,
            exit: false,
            settings,
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

    /// Full signaling room id
    room_id: SignalingRoomId,

    /// User behind the participant or Guest
    participant: api::Participant<User>,

    /// The role of the participant inside the room
    role: Role,

    /// The control data. Initialized when frontend send join
    control_data: Option<ControlData>,

    /// Websocket abstraction which connects the to the websocket actor
    ws: Ws,

    /// All registered and initialized modules
    modules: Modules,
    events: SelectAll<AnyStream>,

    /// Redis connection manager
    redis_conn: ConnectionManager,

    /// RabbitMQ queue consumer for this participant, will contain any events about room and
    /// participant changes
    consumer: lapin::Consumer,
    consumer_delegated: bool,

    /// RabbitMQ channel to send events
    rabbit_mq_channel: lapin::Channel,

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

impl Runner {
    #[allow(clippy::too_many_arguments)]
    pub fn builder(
        runner_id: Uuid,
        id: ParticipantId,
        room: Room,
        breakout_room: Option<BreakoutRoomId>,
        participant: api::Participant<User>,
        protocol: &'static str,
        db: Arc<Db>,
        authz: Arc<Authz>,
        redis_conn: ConnectionManager,
        rabbitmq_channel: lapin::Channel,
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
            api::Participant::Guest | api::Participant::Sip => Role::Guest,
        };

        Builder {
            runner_id,
            id,
            room,
            breakout_room,
            participant,
            role,
            protocol,
            modules: Default::default(),
            rabbitmq_exchanges: vec![],
            rabbitmq_bindings: vec![],
            events: SelectAll::new(),
            db,
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
    pub async fn destroy(mut self) {
        self.delegate_consumer();

        // Cancel subscription to not poison the rabbitmq channel with unacknowledged messages
        if let Err(e) = self
            .rabbit_mq_channel
            .basic_cancel(self.consumer.tag().as_str(), Default::default())
            .await
        {
            log::error!("Failed to cancel consumer, {}", e);
        }

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
                return;
            }
            Err(r3dlock::Error::CouldNotAcquireLock) => {
                log::error!("Failed to acquire r3dlock, contention too high");
                return;
            }
            Err(r3dlock::Error::FailedToUnlock | r3dlock::Error::AlreadyExpired) => {
                unreachable!()
            }
        };

        if let Err(e) = storage::set_attribute(
            &mut self.redis_conn,
            self.room_id,
            self.id,
            "left_at",
            Timestamp::now(),
        )
        .await
        {
            log::error!("Failed to set left_at attribute, {:?}", e);
        }

        let destroy_room =
            match storage::participants_all_left(&mut self.redis_conn, self.room_id).await {
                Ok(destroy_room) => destroy_room,
                Err(e) => {
                    log::error!(
                        "failed to check if all participants have left the room, {:?}",
                        e
                    );
                    false
                }
            };

        let ctx = DestroyContext {
            redis_conn: &mut self.redis_conn,
            destroy_room,
        };

        self.modules.destroy(ctx).await;

        if destroy_room {
            if let Err(e) = self.cleanup_redis().await {
                log::error!("Failed to remove all control attributes, {}", e);
            }
        }

        if let Err(e) = room_guard.unlock(&mut self.redis_conn).await {
            log::error!("Failed to unlock set_guard r3dlock, {}", e);
        }

        if self.control_data.is_some() {
            self.rabbitmq_publish_control(Timestamp::now(), None, rabbitmq::Message::Left(self.id))
                .await;
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
            Err(e) => log::error!("failed to remove participant id, {}", e),
        }
    }

    /// Remove all room and control module related data from redis  
    async fn cleanup_redis(&mut self) -> Result<()> {
        storage::remove_participant_set(&mut self.redis_conn, self.room_id).await?;
        storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "display_name").await?;
        storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "joined_at").await?;
        storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "left_at").await?;
        storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "hand_is_up").await?;
        storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "hand_updated_at")
            .await?;
        storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "kind").await?;
        storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "user_id").await
    }

    /// Runs the runner until the peer closes its websocket connection or a fatal error occurres.
    pub async fn run(mut self) {
        while matches!(self.ws.state, State::Open) {
            if self.exit && matches!(self.ws.state, State::Open) {
                // This case handles exit on errors unrelated to websocket or controller shutdown
                self.ws.close(CloseCode::Abnormal).await;
            }

            tokio::select! {
                res = self.ws.receive() => {
                    match res {
                        Ok(Some(msg)) => self.handle_ws_message(msg).await,
                        Ok(None) => {
                            // Ws was in closing state, runner will now exit gracefully
                        }
                        Err(e) => {
                            // Ws is now going to be in error state and cause the runner to exit
                            log::error!("Failed to receive ws message for participant {}, {}", self.id ,e);
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

        self.destroy().await
    }

    #[tracing::instrument(skip(self, message), fields(id = %self.id))]
    async fn handle_ws_message(&mut self, message: Message) {
        log::trace!("Received websocket message {:?}", message);

        let value: Result<Namespaced<'_, Value>, _> = match message {
            Message::Text(ref text) => serde_json::from_str(text),
            Message::Binary(ref binary) => serde_json::from_slice(binary),
            _ => unreachable!(),
        };

        let namespaced = match value {
            Ok(value) => value,
            Err(e) => {
                log::error!("Failed to parse namespaced message, {}", e);

                self.ws
                    .send(Message::Text(error("invalid json message")))
                    .await;

                return;
            }
        };
        let timestamp = Timestamp::now();

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

                    self.ws
                        .send(Message::Text(error("invalid json payload")))
                        .await;
                }
            }
            // Do not handle any other messages than control-join before joined
        } else if self.control_data.is_some() {
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
                    self.ws
                        .send(Message::Text(error("unknown namespace")))
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
        let settings = self.settings.load();

        match msg {
            incoming::Message::Join(join) => {
                if join.display_name.is_empty() {
                    self.ws
                        .send(Message::Text(
                            NamespacedOutgoing {
                                namespace: NAMESPACE,
                                timestamp,
                                payload: outgoing::Message::Error {
                                    text: "invalid username",
                                },
                            }
                            .to_json(),
                        ))
                        .await;

                    return Ok(());
                }

                let mut lock = storage::room_mutex(self.room_id);

                let guard = lock.lock(&mut self.redis_conn).await?;

                let res = self.join_room_locked(timestamp, &join.display_name).await;

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
                    match self.build_participant(id).await {
                        Ok(participant) => participants.push(participant),
                        Err(e) => log::error!("Failed to build participant {}, {}", id, e),
                    };
                }

                let avatar_url = match &self.participant {
                    api::Participant::User(user) => Some(format!(
                        "{}{:x}",
                        settings.avatar.libravatar_url,
                        md5::compute(&user.email)
                    )),
                    _ => None,
                };

                let control_data = ControlData {
                    display_name: join.display_name.clone(),
                    avatar_url: avatar_url.clone(),
                    participation_kind: match &self.participant {
                        api::Participant::User(_) => ParticipationKind::User,
                        api::Participant::Guest => ParticipationKind::Guest,
                        api::Participant::Sip => ParticipationKind::Sip,
                    },
                    joined_at: timestamp,
                    hand_is_up: false,
                    hand_updated_at: timestamp,
                    left_at: None,
                };

                let mut module_data = HashMap::new();

                let actions = self
                    .handle_module_broadcast_event(
                        timestamp,
                        DynBroadcastEvent::Joined(
                            &control_data,
                            &mut module_data,
                            &mut participants,
                        ),
                        false,
                    )
                    .await;

                self.control_data = Some(control_data);

                self.ws
                    .send(Message::Text(
                        NamespacedOutgoing {
                            namespace: NAMESPACE,
                            timestamp,
                            payload: outgoing::Message::JoinSuccess(outgoing::JoinSuccess {
                                id: self.id,
                                display_name: join.display_name,
                                avatar_url,
                                role: self.role,
                                module_data,
                                participants,
                            }),
                        }
                        .to_json(),
                    ))
                    .await;

                self.rabbitmq_publish_control(timestamp, None, rabbitmq::Message::Joined(self.id))
                    .await;

                self.handle_module_requested_actions(timestamp, actions)
                    .await;
            }
            incoming::Message::RaiseHand => {
                storage::AttrPipeline::new(self.room_id, self.id)
                    .set("hand_is_up", true)
                    .set("hand_updated_at", timestamp)
                    .query_async(&mut self.redis_conn)
                    .await?;

                let actions = self
                    .handle_module_broadcast_event(timestamp, DynBroadcastEvent::RaiseHand, true)
                    .await;

                self.handle_module_requested_actions(timestamp, actions)
                    .await;
            }
            incoming::Message::LowerHand => {
                storage::AttrPipeline::new(self.room_id, self.id)
                    .set("hand_is_up", false)
                    .set("hand_updated_at", timestamp)
                    .query_async(&mut self.redis_conn)
                    .await?;

                let actions = self
                    .handle_module_broadcast_event(timestamp, DynBroadcastEvent::LowerHand, true)
                    .await;

                self.handle_module_requested_actions(timestamp, actions)
                    .await;
            }
        }

        Ok(())
    }

    async fn join_room_locked(
        &mut self,
        timestamp: Timestamp,
        display_name: &str,
    ) -> Result<Vec<ParticipantId>> {
        let mut pipe_attrs = storage::AttrPipeline::new(self.room_id, self.id);

        match &self.participant {
            api::Participant::User(ref user) => {
                let avatar_url = format!(
                    "{}{:x}",
                    self.settings.load().avatar.libravatar_url,
                    md5::compute(&user.email)
                );

                pipe_attrs
                    .set("kind", ParticipationKind::User)
                    .set("avatar_url", avatar_url)
                    .set("user_id", user.id);
            }
            api::Participant::Guest => {
                pipe_attrs.set("kind", ParticipationKind::Guest);
            }
            api::Participant::Sip => {
                pipe_attrs.set("kind", ParticipationKind::Sip);
            }
        }

        pipe_attrs
            .set("hand_is_up", false)
            .set("hand_updated_at", timestamp)
            .set("display_name", display_name)
            .set("joined_at", timestamp)
            .del("left_at")
            .query_async(&mut self.redis_conn)
            .await?;

        let participants = storage::get_all_participants(&mut self.redis_conn, self.room_id)
            .await
            .context("Failed to get all active participants")?;

        let num_added =
            storage::add_participant_to_set(&mut self.redis_conn, self.room_id, self.id)
                .await
                .context("Failed to add self to participants set")?;

        // Check that SADD doesn't return 0. That would mean that the participant id would be a
        // duplicate which cannot be allowed. Since this should never happen just error and exit.
        if num_added == 0 {
            bail!("participant-id is already taken inside participant set");
        }

        Ok(participants)
    }

    async fn build_participant(&mut self, id: ParticipantId) -> Result<Participant> {
        let mut participant = outgoing::Participant {
            id,
            module_data: Default::default(),
        };

        #[allow(clippy::type_complexity)]
        let (
            display_name,
            avatar_url,
            joined_at,
            left_at,
            hand_is_up,
            hand_updated_at,
            participation_kind,
        ): (
            Option<String>,
            Option<String>,
            Option<Timestamp>,
            Option<Timestamp>,
            Option<bool>,
            Option<Timestamp>,
            Option<ParticipationKind>,
        ) = storage::AttrPipeline::new(self.room_id, id)
            .get("display_name")
            .get("avatar_url")
            .get("joined_at")
            .get("left_at")
            .get("hand_is_up")
            .get("hand_updated_at")
            .get("kind")
            .query_async(&mut self.redis_conn)
            .await?;

        if display_name.is_none()
            || joined_at.is_none()
            || hand_is_up.is_none()
            || hand_updated_at.is_none()
        {
            log::error!("failed to fetch some attribute, using fallback defaults");
        }

        participant.module_data.insert(
            NAMESPACE,
            serde_json::to_value(ControlData {
                display_name: display_name.unwrap_or_else(|| "Participant".into()),
                avatar_url,
                participation_kind: participation_kind.unwrap_or(ParticipationKind::Guest),
                hand_is_up: hand_is_up.unwrap_or_default(),
                hand_updated_at: hand_updated_at.unwrap_or_else(Timestamp::unix_epoch),
                joined_at: joined_at.unwrap_or_else(Timestamp::unix_epoch),
                // no default for left_at. If its not found by error,
                // worst case we have a ghost participant,
                left_at,
            })
            .expect("Failed to convert ControlData to serde_json::Value"),
        );

        Ok(participant)
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
            if self.control_data.is_none() {
                return;
            }
            let timestamp: Timestamp = delivery.properties.timestamp().map(|value| chrono::Utc.timestamp(value as i64, 0)).unwrap_or_else(|| {
                log::warn!("Got RabbitMQ message without timestamp. Creating current timestamp as fallback.");
                chrono::Utc::now()
            }
            ).into();

            let namespaced = match serde_json::from_slice::<Namespaced<Value>>(&delivery.data) {
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
            } else {
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
        } else if matches!(self.participant, api::Participant::User(ref user)
            if delivery.routing_key.as_str() == user_update::routing_key(user.id)
        ) {
            let user_update::Message { groups } = match serde_json::from_slice(&delivery.data) {
                Ok(message) => message,
                Err(e) => {
                    log::error!("Failed to parse user_update message, {}", e);
                    return;
                }
            };

            if groups {
                // TODO groups have changed, inspect permissions
                // Workaround since this is an edge-case: kill runner
                log::debug!("User groups changed, exiting");
                self.exit = true;
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
                if self.id == id {
                    return Ok(());
                }

                let mut participant = self.build_participant(id).await?;

                let actions = self
                    .handle_module_broadcast_event(
                        timestamp,
                        DynBroadcastEvent::ParticipantJoined(&mut participant),
                        false,
                    )
                    .await;

                self.ws
                    .send(Message::Text(
                        NamespacedOutgoing {
                            namespace: NAMESPACE,
                            timestamp,
                            payload: outgoing::Message::Joined(participant),
                        }
                        .to_json(),
                    ))
                    .await;

                self.handle_module_requested_actions(timestamp, actions)
                    .await;
            }
            rabbitmq::Message::Left(id) => {
                if self.id == id {
                    return Ok(());
                }

                let actions = self
                    .handle_module_broadcast_event(
                        timestamp,
                        DynBroadcastEvent::ParticipantLeft(id),
                        false,
                    )
                    .await;

                self.ws
                    .send(Message::Text(
                        NamespacedOutgoing {
                            namespace: NAMESPACE,
                            timestamp,
                            payload: outgoing::Message::Left(outgoing::AssociatedParticipant {
                                id,
                            }),
                        }
                        .to_json(),
                    ))
                    .await;

                self.handle_module_requested_actions(timestamp, actions)
                    .await;
            }
            rabbitmq::Message::Update(id) => {
                if self.id == id {
                    return Ok(());
                }

                let mut participant = self.build_participant(id).await?;

                let actions = self
                    .handle_module_broadcast_event(
                        timestamp,
                        DynBroadcastEvent::ParticipantUpdated(&mut participant),
                        false,
                    )
                    .await;

                self.ws
                    .send(Message::Text(
                        NamespacedOutgoing {
                            namespace: NAMESPACE,
                            timestamp,
                            payload: outgoing::Message::Update(participant),
                        }
                        .to_json(),
                    ))
                    .await;

                self.handle_module_requested_actions(timestamp, actions)
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
        let message = Namespaced {
            namespace: NAMESPACE,
            payload: message,
        };

        let routing_key = if let Some(recipient) = recipient {
            Cow::Owned(rabbitmq::room_participant_routing_key(recipient))
        } else {
            Cow::Borrowed(rabbitmq::room_all_routing_key())
        };

        self.rabbitmq_publish(timestamp, None, &routing_key, message.to_json())
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
            .rabbit_mq_channel
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
            timestamp,
            ws_messages: &mut ws_messages,
            rabbitmq_publish: &mut rabbitmq_publish,
            redis_conn: &mut self.redis_conn,
            events: &mut self.events,
            invalidate_data: &mut invalidate_data,
            exit: &mut exit,
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
            timestamp,
            ws_messages: &mut ws_messages,
            rabbitmq_publish: &mut rabbitmq_publish,
            redis_conn: &mut self.redis_conn,
            events: &mut self.events,
            invalidate_data: &mut invalidate_data,
            exit: &mut exit,
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
                Some(&publish.exchange),
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
}

#[must_use]
struct ModuleRequestedActions {
    ws_messages: Vec<Message>,
    rabbitmq_publish: Vec<RabbitMqPublish>,
    invalidate_data: bool,
    exit: Option<CloseCode>,
}

fn error(text: &str) -> ByteString {
    NamespacedOutgoing {
        namespace: "error",
        timestamp: Timestamp::now(),
        payload: text,
    }
    .to_json()
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
