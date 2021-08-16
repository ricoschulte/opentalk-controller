use super::modules::{
    AnyStream, DynBroadcastEvent, DynEventCtx, DynTargetedEvent, Modules, NoSuchModuleError,
};
use super::{
    DestroyContext, Namespaced, RabbitMqBinding, RabbitMqExchange, RabbitMqPublish, WebSocket,
};
use crate::api::signaling::ws_modules::control::outgoing::Participant;
use crate::api::signaling::ws_modules::control::{incoming, outgoing, rabbitmq, storage};
use crate::api::signaling::{ParticipantId, Role};
use crate::db::rooms::Room;
use crate::db::users::User;
use crate::db::DbInterface;
use crate::ha_sync::user_update;
use anyhow::{bail, Context, Result};
use async_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use async_tungstenite::tungstenite::protocol::CloseFrame;
use async_tungstenite::tungstenite::Message;
use futures::stream::SelectAll;
use futures::SinkExt;
use lapin::message::DeliveryResult;
use lapin::options::QueueDeclareOptions;
use lapin::ExchangeKind;
use redis::aio::ConnectionManager;
use serde::Serialize;
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::{sleep, Sleep};
use tokio_stream::StreamExt;

const WS_TIMEOUT: Duration = Duration::from_secs(20);

const NAMESPACE: &str = "control";

/// Builder to the runner type.
/// Passed into [`ModuleBuilder::build`] function to create [`InitContext`]
pub struct Builder {
    pub(super) id: ParticipantId,
    pub(super) room: Room,
    pub(super) user: User,
    pub(super) role: Role,
    pub(super) protocol: &'static str,
    pub(super) modules: Modules,
    pub(super) rabbitmq_exchanges: Vec<RabbitMqExchange>,
    pub(super) rabbitmq_bindings: Vec<RabbitMqBinding>,
    pub(super) events: SelectAll<AnyStream>,
    pub(super) db: Arc<DbInterface>,
    pub(super) redis_conn: ConnectionManager,
    pub(super) rabbitmq_channel: lapin::Channel,
}

impl Builder {
    /// Abort the building process and destroy all already built modules
    pub async fn abort(mut self) {
        let ctx = DestroyContext {
            redis_conn: &mut self.redis_conn,
            // We haven't joined yet
            destroy_room: false,
        };

        self.modules.destroy(ctx).await
    }

    /// Build to runner from the data inside the builder and provided websocket
    pub async fn build(
        mut self,
        websocket: WebSocket,
        shutdown_sig: broadcast::Receiver<()>,
    ) -> Result<Runner> {
        // ==== CONTROL MODULE DECLARATIONS ====

        // The name of the room exchange
        let room_exchange = rabbitmq::room_exchange_name(self.room.uuid);

        // Routing key to receive messages directed to all participants
        let all_routing_key = rabbitmq::room_all_routing_key();

        // Routing key which is needed to receive messages to this participant
        let self_routing_key = rabbitmq::room_participant_routing_key(self.id);

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
            // routing keys in global exchanges are ignored
            routing_key: all_routing_key.into(),
            exchange: room_exchange.clone(),
            options: Default::default(),
        });

        // Add binding to messages directed to self_key (the participant)
        self.rabbitmq_bindings.insert(
            0,
            RabbitMqBinding {
                routing_key: self_routing_key.clone(),
                exchange: room_exchange.clone(),
                options: Default::default(),
            },
        );

        // ==== BEGIN GENERIC SETUP ====

        // Create the queue for this participant
        let queue_options = QueueDeclareOptions {
            auto_delete: true,
            ..Default::default()
        };

        let queue = self
            .rabbitmq_channel
            .queue_declare(
                &format!("{}.{}", room_exchange, self_routing_key),
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

        // Binding outside the loop since the routing key contains the user-id
        // and thus should not appear in the logs
        self.rabbitmq_channel
            .queue_bind(
                queue.name().as_str(),
                user_update::EXCHANGE,
                &user_update::routing_key(self.user.id),
                Default::default(),
                Default::default(),
            )
            .await?;

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

        Ok(Runner {
            id: self.id,
            room: self.room,
            user: self.user,
            role: self.role,
            control_data: None,
            ws: Ws {
                websocket,
                timeout: Box::pin(sleep(WS_TIMEOUT)),
                awaiting_pong: false,
                state: State::Open,
            },
            modules: self.modules,
            events: self.events,
            redis_conn: self.redis_conn,
            consumer,
            consumer_delegated: false,
            rabbit_mq_channel: self.rabbitmq_channel,
            room_exchange,
            shutdown_sig,
            exit: false,
        })
    }
}

/// The websocket runner
///
/// As root of the websocket-task it is responsible to drive the websocket application,
/// manage setup and teardown of redis storage, RabbitMQ queues and modules.
///
/// Also acts as `control` module which handles participant and room states.
pub struct Runner {
    // participant id that the runner is connected to
    id: ParticipantId,

    // Room the participant is inside
    room: Room,

    // User behind the participant
    user: User,

    // The role of the participant inside the room
    role: Role,

    // The control data. Initialized when frontend send join
    control_data: Option<ControlData>,

    // Websocket abstraction which helps detecting timeouts using regular ping-messages
    ws: Ws,

    // All registered and initialized modules
    modules: Modules,
    events: SelectAll<AnyStream>,

    // Redis connection manager
    redis_conn: ConnectionManager,

    // RabbitMQ queue consumer for this participant, will contain any events about room and
    // participant changes
    consumer: lapin::Consumer,
    consumer_delegated: bool,

    // RabbitMQ channel to send events
    rabbit_mq_channel: lapin::Channel,

    // Name of the rabbitmq room exchange
    room_exchange: String,

    // global application shutdown signal
    shutdown_sig: broadcast::Receiver<()>,

    // When set to true the runner will gracefully exit on next loop
    exit: bool,
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
    pub fn builder(
        id: ParticipantId,
        room: Room,
        user: User,
        protocol: &'static str,
        db: Arc<DbInterface>,
        redis_conn: ConnectionManager,
        rabbitmq_channel: lapin::Channel,
    ) -> Builder {
        let role = if user.id == room.owner {
            Role::Moderator
        } else {
            Role::User
        };

        Builder {
            id,
            room,
            user,
            role,
            protocol,
            modules: Default::default(),
            rabbitmq_exchanges: vec![],
            rabbitmq_bindings: vec![],
            events: SelectAll::new(),
            db,
            redis_conn,
            rabbitmq_channel,
        }
    }

    /// Delegate all messages of this consumer to leave no messages unacknowledged until it is properly destroyed
    ///
    /// After calling this function the runner will no longer receive rabbitmq messages
    fn delegate_consumer(&mut self) {
        let result = self.consumer.set_delegate(|delivery: DeliveryResult| {
            Box::pin(async move {
                match delivery {
                    Ok(Some((_, delivery))) => {
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

        if let Err(e) = result {
            log::error!("Failed to delegate consumer, {}", e);
        } else {
            self.consumer_delegated = true;
        }
    }

    /// Destroys the runner and all associated resources
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
        let mut set_lock = storage::participant_set_mutex(self.room.uuid);

        let set_guard = match set_lock.lock(&mut self.redis_conn).await {
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

        // Remove participant from set and check if set is empty
        let destroy_room = match storage::remove_participant_from_set(
            &set_guard,
            &mut self.redis_conn,
            self.room.uuid,
            self.id,
        )
        .await
        {
            Ok(n) => n == 0,
            Err(e) => {
                log::error!("Failed to remove participant from room, {}", e);
                // TODO this possibly leaks resources if we actually are the last one in the room
                false
            }
        };

        if let Err(e) =
            storage::remove_all_attributes(&mut self.redis_conn, self.room.uuid, self.id).await
        {
            log::error!("Failed to remove all control attributes, {}", e);
        }

        let ctx = DestroyContext {
            redis_conn: &mut self.redis_conn,
            destroy_room,
        };

        self.modules.destroy(ctx).await;

        if let Err(e) = set_guard.unlock(&mut self.redis_conn).await {
            log::error!("Failed to unlock set_guard r3dlock, {}", e);
        }

        if self.control_data.is_some() {
            self.rabbitmq_publish_control(None, rabbitmq::Message::Left(self.id))
                .await;
        }
    }

    /// Runs the runner until the peer closes its websocket connection or a fatal error occurres.
    pub async fn run(mut self) {
        while matches!(self.ws.state, State::Open | State::Closing) {
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
                        Some(Ok((channel, delivery))) => self.handle_rabbitmq_msg(channel, delivery).await,
                        _ => {
                            // None or Some(Err(_)), either way its an error to us
                            log::error!("Failed to receive RabbitMQ message, exiting");
                            self.exit = true;
                        }
                    }
                }
                Some((namespace, any)) = self.events.next() => {
                    self.handle_module_targeted_event(namespace, DynTargetedEvent::Ext(any))
                        .await
                        .expect("Should not get events from unknown modules");
                }
                _ = self.shutdown_sig.recv() => {
                    self.ws.close(CloseCode::Away).await;
                }
            }
        }

        self.handle_module_broadcast_event(DynBroadcastEvent::Leaving)
            .await;

        log::debug!("Stopping ws-runner task for participant {}", self.id);

        self.destroy().await
    }

    async fn handle_ws_message(&mut self, message: Message) {
        let value: Result<Namespaced<'_, Value>, _> = match message {
            Message::Text(ref text) => serde_json::from_str(text),
            Message::Binary(ref binary) => serde_json::from_slice(binary),
            Message::Ping(_) | Message::Pong(_) | Message::Close(_) => {
                unreachable!()
            }
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

        if namespaced.namespace == NAMESPACE {
            match serde_json::from_value(namespaced.payload) {
                Ok(msg) => {
                    if let Err(e) = self.handle_control_msg(msg).await {
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
            if let Err(NoSuchModuleError(())) = self
                .handle_module_targeted_event(
                    namespaced.namespace,
                    DynTargetedEvent::WsMessage(namespaced.payload),
                )
                .await
            {
                self.ws
                    .send(Message::Text(error("unknown namespace")))
                    .await;
            }
        }
    }

    async fn handle_control_msg(&mut self, msg: incoming::Message) -> Result<()> {
        match msg {
            incoming::Message::Join(join) => {
                if join.display_name.is_empty() {
                    self.ws
                        .send(Message::Text(
                            Namespaced {
                                namespace: NAMESPACE,
                                payload: outgoing::Message::Error {
                                    text: "invalid username",
                                },
                            }
                            .to_json(),
                        ))
                        .await;

                    return Ok(());
                }

                storage::set_attribute(
                    &mut self.redis_conn,
                    self.room.uuid,
                    self.id,
                    "display_name",
                    &join.display_name,
                )
                .await
                .context("Failed to set display_name")?;

                self.control_data = Some(ControlData {
                    display_name: join.display_name,
                });

                let participant_set =
                    storage::get_all_participants(&mut self.redis_conn, self.room.uuid)
                        .await
                        .context("Failed to get all active participants")?;

                storage::add_participant_to_set(&mut self.redis_conn, self.room.uuid, self.id)
                    .await
                    .context("Failed to add self to participants set")?;

                let mut participants = vec![];

                for id in participant_set {
                    match self.build_participant(id).await {
                        Ok(participant) => participants.push(participant),
                        Err(e) => log::error!("Failed to build participant {}, {}", id, e),
                    };
                }

                let mut module_data = HashMap::new();

                self.handle_module_broadcast_event(DynBroadcastEvent::Joined(
                    &mut module_data,
                    &mut participants,
                ))
                .await;

                self.ws
                    .send(Message::Text(
                        Namespaced {
                            namespace: NAMESPACE,
                            payload: outgoing::Message::JoinSuccess(outgoing::JoinSuccess {
                                id: self.id,
                                role: self.role,
                                module_data,
                                participants,
                            }),
                        }
                        .to_json(),
                    ))
                    .await;

                self.rabbitmq_publish_control(None, rabbitmq::Message::Joined(self.id))
                    .await;
            }
        }

        Ok(())
    }

    async fn build_participant(&mut self, id: ParticipantId) -> Result<Participant> {
        let mut participant = outgoing::Participant {
            id,
            module_data: Default::default(),
        };

        let display_name: String =
            storage::get_attribute(&mut self.redis_conn, self.room.uuid, id, "display_name")
                .await?;

        participant.module_data.insert(
            NAMESPACE,
            serde_json::to_value(ControlData { display_name })
                .expect("Failed to convert ControlData to serde_json::Value"),
        );

        Ok(participant)
    }

    async fn handle_rabbitmq_msg(&mut self, _: lapin::Channel, delivery: lapin::message::Delivery) {
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

                if let Err(e) = self.handle_rabbitmq_control_msg(msg).await {
                    log::error!("Failed to handle incoming rabbitmq control msg, {}", e);
                    return;
                }
            } else if let Err(NoSuchModuleError(())) = self
                .handle_module_targeted_event(
                    namespaced.namespace,
                    DynTargetedEvent::RabbitMqMessage(namespaced.payload),
                )
                .await
            {
                log::warn!("Got invalid rabbit-mq message");
            }
        } else if delivery.routing_key.as_str() == user_update::routing_key(self.user.id) {
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

    async fn handle_rabbitmq_control_msg(&mut self, msg: rabbitmq::Message) -> Result<()> {
        log::debug!("Received RabbitMQ control message {:?}", msg);

        match msg {
            rabbitmq::Message::Joined(id) => {
                if self.id == id {
                    return Ok(());
                }

                let mut participant = self.build_participant(id).await?;

                self.handle_module_broadcast_event(DynBroadcastEvent::ParticipantJoined(
                    &mut participant,
                ))
                .await;

                self.ws
                    .send(Message::Text(
                        Namespaced {
                            namespace: NAMESPACE,
                            payload: outgoing::Message::Joined(participant),
                        }
                        .to_json(),
                    ))
                    .await;
            }
            rabbitmq::Message::Left(id) => {
                if self.id == id {
                    return Ok(());
                }

                self.handle_module_broadcast_event(DynBroadcastEvent::ParticipantLeft(id))
                    .await;

                self.ws
                    .send(Message::Text(
                        Namespaced {
                            namespace: NAMESPACE,
                            payload: outgoing::Message::Left(outgoing::AssociatedParticipant {
                                id,
                            }),
                        }
                        .to_json(),
                    ))
                    .await;
            }
            rabbitmq::Message::Update(id) => {
                if self.id == id {
                    return Ok(());
                }

                let mut participant = self.build_participant(id).await?;

                self.handle_module_broadcast_event(DynBroadcastEvent::ParticipantUpdated(
                    &mut participant,
                ))
                .await;

                self.ws
                    .send(Message::Text(
                        Namespaced {
                            namespace: NAMESPACE,
                            payload: outgoing::Message::Update(participant),
                        }
                        .to_json(),
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

        self.rabbitmq_publish(None, &routing_key, message.to_json())
            .await;
    }

    /// Publish a rabbitmq message
    ///
    /// If exchange is `None`, `self.room_exchange` will be used.
    async fn rabbitmq_publish(
        &mut self,
        exchange: Option<&str>,
        routing_key: &str,
        message: String,
    ) {
        log::trace!(
            "Publish to rabbitmq exchange={:?} routing_key={:?} message={:?}",
            exchange,
            routing_key,
            message
        );

        if let Err(e) = self
            .rabbit_mq_channel
            .basic_publish(
                exchange.unwrap_or(&self.room_exchange),
                routing_key,
                Default::default(),
                message.into_bytes(),
                Default::default(),
            )
            .await
        {
            log::error!("Failed to send message over rabbitmq, {}", e);
            self.exit = true;
            return;
        }
    }

    /// Dispatch owned event to a single module
    async fn handle_module_targeted_event(
        &mut self,
        module: &str,
        dyn_event: DynTargetedEvent,
    ) -> Result<(), NoSuchModuleError> {
        let mut ws_messages = vec![];
        let mut rabbitmq_publish = vec![];
        let mut invalidate_data = false;

        let ctx = DynEventCtx {
            id: self.id,
            ws_messages: &mut ws_messages,
            rabbitmq_publish: &mut rabbitmq_publish,
            redis_conn: &mut self.redis_conn,
            invalidate_data: &mut invalidate_data,
        };

        self.modules
            .on_event_targeted(ctx, module, dyn_event)
            .await?;

        self.handle_module_requested_actions(ws_messages, rabbitmq_publish, invalidate_data)
            .await;

        Ok(())
    }

    /// Dispatch copyable event to all modules
    async fn handle_module_broadcast_event(&mut self, dyn_event: DynBroadcastEvent<'_>) {
        let mut ws_messages = vec![];
        let mut rabbitmq_publish = vec![];
        let mut invalidate_data = false;

        let ctx = DynEventCtx {
            id: self.id,
            ws_messages: &mut ws_messages,
            rabbitmq_publish: &mut rabbitmq_publish,
            redis_conn: &mut self.redis_conn,
            invalidate_data: &mut invalidate_data,
        };

        self.modules.on_event_broadcast(ctx, dyn_event).await;

        self.handle_module_requested_actions(ws_messages, rabbitmq_publish, invalidate_data)
            .await;
    }

    /// Modules can request certain actions via the module context (e.g send websocket msg)
    /// these are executed here
    async fn handle_module_requested_actions(
        &mut self,
        ws_messages: Vec<Message>,
        rabbitmq_publish: Vec<RabbitMqPublish>,
        invalidate_data: bool,
    ) {
        for ws_message in ws_messages {
            self.ws.send(ws_message).await;
        }

        for publish in rabbitmq_publish {
            self.rabbitmq_publish(
                Some(&publish.exchange),
                &publish.routing_key,
                publish.message,
            )
            .await;
        }

        if invalidate_data {
            self.rabbitmq_publish_control(None, rabbitmq::Message::Update(self.id))
                .await;
        }
    }
}

fn error(text: &str) -> String {
    Namespaced {
        namespace: "error",
        payload: text,
    }
    .to_json()
}

#[derive(Serialize)]
struct ControlData {
    display_name: String,
}

/// Helper websocket abstraction that pings the participants in regular intervals
struct Ws {
    websocket: WebSocket,
    // Timeout trigger
    timeout: Pin<Box<Sleep>>,

    awaiting_pong: bool,

    state: State,
}

enum State {
    Open,
    Closing,
    Closed,
    Error,
}

impl Ws {
    /// Send message via websocket
    async fn send(&mut self, message: Message) {
        log::trace!("Send message to websocket: {:?}", message);

        if let Err(e) = self.websocket.send(message).await {
            log::error!("Failed to send websocket message, {}", e);
            self.state = State::Error;
        }
    }

    /// Close the websocket connection if needed
    async fn close(&mut self, code: CloseCode) {
        if !matches!(self.state, State::Open) {
            return;
        }

        let frame = CloseFrame {
            code,
            reason: "".into(),
        };

        if let Err(e) = self.websocket.close(Some(frame)).await {
            log::error!("Failed to close websocket, {}", e);
            self.state = State::Error;
        } else {
            self.state = State::Closing;
            self.timeout.set(sleep(WS_TIMEOUT));
        }
    }

    /// Receive a message from the websocket
    ///
    /// Sends a health check ping message every WS_TIMEOUT.
    async fn receive(&mut self) -> Result<Option<Message>> {
        loop {
            tokio::select! {
                message = self.websocket.next() => {
                    match message {
                        Some(Ok(msg)) => {
                            if matches!(self.state, State::Open) {
                                // Received a message, reset timeout.
                                self.timeout.set(sleep(WS_TIMEOUT));
                            }

                            match msg {
                                Message::Ping(ping) => self.send(Message::Pong(ping)).await,
                                Message::Pong(_) => self.awaiting_pong = false,
                                Message::Close(_) => self.state = State::Closing,
                                msg => return Ok(Some(msg))
                            }
                        }
                        Some(Err(e)) => {
                            self.state = State::Error;
                            bail!(e);
                        }
                        None => if matches!(self.state, State::Closing) {
                            self.state = State::Closed;
                            return Ok(None)
                        } else {
                            self.state = State::Error;
                            bail!("WebSocket stream closed unexpectedly")
                        },
                    }
                }
                _ = self.timeout.as_mut() => {
                    if self.awaiting_pong {
                        self.state = State::Error;
                        bail!("Websocket timed out, peer no longer responds");
                    } else {
                        self.timeout.set(sleep(WS_TIMEOUT));
                        self.awaiting_pong = true;

                        self.websocket.send(Message::Ping(vec![])).await?;
                    }
                }
            }
        }
    }
}
