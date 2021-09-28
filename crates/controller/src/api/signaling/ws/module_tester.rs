//! The ModuleTester simulates a runner environment for a specified module.
//!
//! This module is exclusively used for testing and does not contribute to the controllers behavior.
//! As its basically a 'copy' of the [`super::runner::Runner`] it uses a few types from there. Due to
//! visibility restriction of those types, this module is located in the same folder.
//!
//! The idea is to simulate a frontend websocket connection. See the LegalVote integration tests for examples.
use super::modules::AnyStream;
use super::{
    DestroyContext, Event, Namespaced, NamespacedOutgoing, RabbitMqPublish, SignalingModule,
};
use crate::api::signaling::prelude::control::incoming::Join;
use crate::api::signaling::prelude::control::{self, outgoing, storage, ControlData};
use crate::api::signaling::prelude::{BreakoutRoomId, InitContext, ModuleContext};
use crate::api::signaling::ws::runner::NAMESPACE;
use crate::api::signaling::{ParticipantId, Role, SignalingRoomId, Timestamp};
use crate::db::rooms::Room;
use crate::db::users::User;
use crate::db::users::UserId;
use crate::db::DbInterface;
use actix_rt::task::JoinHandle;
use anyhow::{bail, Context, Result};
use async_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use futures::stream::SelectAll;
use redis::aio::ConnectionManager;
use serde_json::Value;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::panic;
use std::sync::Arc;
use std::time::Duration;
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{broadcast, mpsc};
use tokio::task;
use tokio::time::timeout;
use tokio_stream::StreamExt;

/// A module tester that simulates a runner environment for provided module.
///
/// When created, the `ModuleTester` instance acts like a client websocket connection. This means
/// that incoming events like `Join`, `RaiseHand` and `LowerHand` can be sent to the underlying module as well
/// as module specific WebSocket messages. Outgoing messages like `JoinSuccess`, `Joined`, `Left`, etc. can
/// be received via an internal channel. See [`ModuleTester::send_ws_message`] & [`ModuleTester::receive_ws_message`]
/// for more details.
pub struct ModuleTester<M>
where
    M: SignalingModule,
{
    /// The redis interface
    pub redis_conn: ConnectionManager,
    /// The database interface
    pub db_ctx: Arc<DbInterface>,
    /// The room that the users are inside
    room: Room,
    /// Optional breakout room id
    breakout_room: Option<BreakoutRoomId>,

    /// A map of RunnerInterfaces with their JoinHandle, each for a participant
    runner_interfaces: HashMap<ParticipantId, (RunnerInterface<M>, JoinHandle<()>)>,
    /// A rabbitmq broadcast channel that mocks a rabbitmq connection
    rabbitmq_sender: broadcast::Sender<RabbitMqPublish>,
}

impl<M> ModuleTester<M>
where
    M: SignalingModule,
{
    /// Create a new ModuleTester instance
    pub fn new(db_ctx: Arc<DbInterface>, redis_conn: ConnectionManager, room: Room) -> Self {
        let (rabbitmq_sender, _) = broadcast::channel(10);

        Self {
            redis_conn,
            db_ctx,
            room,
            // todo: add breakout room support
            breakout_room: None,
            runner_interfaces: HashMap::new(),
            rabbitmq_sender,
        }
    }

    /// Join the ModuleTester as the specified user
    ///
    /// This is the equivalent of joining a room in the real controller. Spawns a underlying runner task that
    /// can send and receive WebSocket messages.
    pub async fn join_user(
        &mut self,
        participant_id: ParticipantId,
        user: User,
        role: Role,
        display_name: &str,
        params: M::Params,
    ) -> Result<()> {
        let (client_interface, runner_interface) = create_interfaces::<M>().await;

        let user_id = user.id;

        let runner = MockRunner::<M>::new(
            participant_id,
            self.room.clone(),
            self.breakout_room,
            user,
            role,
            self.db_ctx.clone(),
            self.redis_conn.clone(),
            params,
            client_interface,
            self.rabbitmq_sender.clone(),
        )
        .await?;

        storage::set_attribute(
            &mut self.redis_conn,
            runner.room_id,
            participant_id,
            "user_id",
            user_id,
        )
        .await?;

        let runner_handle = task::spawn_local(runner.run());

        runner_interface.ws.send(WsMessageIncoming::Control(
            control::incoming::Message::Join(Join {
                display_name: display_name.into(),
            }),
        ))?;

        self.runner_interfaces
            .insert(participant_id, (runner_interface, runner_handle));

        Ok(())
    }

    /// Send a module specific WebSocket message to the underlying module that is mapped to `participant_id`.
    ///
    /// # Note
    /// WebSocket control messages (e.g. [`RaiseHand`](control::incoming::Message::RaiseHand),
    /// [`LowerHand`](control::incoming::Message::LowerHand)) have to be sent via their respective helper function.
    pub fn send_ws_message(
        &self,
        participant_id: &ParticipantId,
        message: M::Incoming,
    ) -> Result<()> {
        let (interface, ..) = self
            .runner_interfaces
            .get(participant_id)
            .expect("User {} does not exist in module tester");

        interface.ws.send(WsMessageIncoming::Module(message))?;

        Ok(())
    }

    /// Receive a WebSocket message from the underlying Module that is mapped to `participant_id`
    ///
    ///
    /// This function will yield when there is no available message and timeout after two seconds.
    /// When a longer timeout is required, use [`ModuleTester::receive_ws_message_with_specific_timeout`]
    ///
    /// # Returns
    /// - Ok([`WsMessageOutgoing`]) when a message is available within the timeout window.
    /// - Err([`anyhow::Error`]) on timeout or when the internal channel has been closed.
    pub async fn receive_ws_message(
        &mut self,
        participant_id: &ParticipantId,
    ) -> Result<WsMessageOutgoing<M>> {
        self.receive_ws_message_override_timeout(participant_id, Duration::from_secs(2))
            .await
    }

    /// Receive a WebSocket message from the underlying Module that is mapped to `participant_id`
    ///
    /// Behaves like [`ModuleTester::receive_ws_message`] but allows a custom timeout.
    pub async fn receive_ws_message_override_timeout(
        &mut self,
        participant_id: &ParticipantId,
        timeout_duration: Duration,
    ) -> Result<WsMessageOutgoing<M>> {
        let interface = self.get_runner_interface(participant_id)?;

        match timeout(timeout_duration, interface.ws.recv()).await? {
            Some(message) => Ok(message),
            None => bail!("Failed to receive ws message in module tester"),
        }
    }

    /// Send a [`RaiseHand`](control::incoming::Message::RaiseHand) control message to the module/runner.
    pub fn raise_hand(&mut self, participant_id: &ParticipantId) -> Result<()> {
        let interface = self.get_runner_interface(participant_id)?;

        interface.ws.send(WsMessageIncoming::Control(
            control::incoming::Message::RaiseHand,
        ))
    }

    /// Send a [`LowerHand`](control::incoming::Message::LowerHand) control message to the module/runner.
    pub fn lower_hand(&mut self, participant_id: &ParticipantId) -> Result<()> {
        let interface = self.get_runner_interface(participant_id)?;

        interface.ws.send(WsMessageIncoming::Control(
            control::incoming::Message::LowerHand,
        ))
    }

    /// Close the WebSocket channel and leave the room with the participant
    ///
    /// # Panics
    /// When the participants runner panicked
    async fn leave(&mut self, participant_id: &ParticipantId) -> Result<()> {
        let (interface, handle) = self.get_runner(participant_id)?;

        interface.ws.send(WsMessageIncoming::CloseWs)?;

        // expect the runner to shutdown within 3 seconds
        match timeout(Duration::from_secs(3), handle)
            .await
            .context("Failed to shutdown MockRunner within 3 seconds after leave event")?
        {
            Ok(_) => {
                self.runner_interfaces.remove(participant_id);
                Ok(())
            }
            Err(join_error) => {
                if join_error.is_panic() {
                    panic::resume_unwind(join_error.into_panic());
                }

                bail!(join_error);
            }
        }
    }

    /// Get the [`RunnerInterface`] of the runner that is mapped to `participant_id`
    fn get_runner_interface(
        &mut self,
        participant_id: &ParticipantId,
    ) -> Result<&mut RunnerInterface<M>> {
        Ok(&mut self.get_runner(participant_id)?.0)
    }

    /// Get the [`RunnerInterface`] & [`JoinHandle`] of the runner that is mapped to `participant_id`
    fn get_runner(
        &mut self,
        participant_id: &ParticipantId,
    ) -> Result<&mut (RunnerInterface<M>, JoinHandle<()>)> {
        self.runner_interfaces
            .get_mut(participant_id)
            .with_context(|| {
                format!(
                    "Participant {} does not exist in module tester",
                    participant_id
                )
            })
    }

    fn get_participants(&self) -> Vec<ParticipantId> {
        self.runner_interfaces
            .iter()
            .map(|(participant, ..)| *participant)
            .collect()
    }

    /// Shutdown the ModuleTester
    ///
    /// Leave the room with all participants. Continues to unwind panics that happened in any runner.
    pub async fn shutdown(mut self) -> Result<()> {
        let participants = self.get_participants();

        for participant_id in participants {
            self.leave(&participant_id).await?;
        }

        Ok(())
    }
}

/// Acts like a [Runner](super::runner::Runner) for a single specific module.
struct MockRunner<M>
where
    M: SignalingModule,
{
    redis_conn: ConnectionManager,
    room_id: SignalingRoomId,
    room: Room,
    participant_id: ParticipantId,
    user_id: UserId,
    role: Role,
    control_data: Option<ControlData>,
    module: M,
    interface: ClientInterface<M>,
    rabbitmq_sender: broadcast::Sender<RabbitMqPublish>,
    events: SelectAll<AnyStream>,
    exit: bool,
}

#[allow(clippy::too_many_arguments)]
impl<M> MockRunner<M>
where
    M: SignalingModule,
{
    /// Create a new runner and initialize the underlying module.
    async fn new(
        participant_id: ParticipantId,
        mut room: Room,
        breakout_room: Option<BreakoutRoomId>,
        mut user: User,
        role: Role,
        db_ctx: Arc<DbInterface>,
        mut redis_conn: ConnectionManager,
        params: M::Params,
        interface: ClientInterface<M>,
        rabbitmq_sender: broadcast::Sender<RabbitMqPublish>,
    ) -> Result<Self> {
        let mut events = SelectAll::new();

        let init_context = InitContext {
            id: participant_id,
            room: &mut room,
            breakout_room,
            user: &mut user,
            role,
            db: &db_ctx,
            rabbitmq_exchanges: &mut vec![],
            rabbitmq_bindings: &mut vec![],
            events: &mut events,
            redis_conn: &mut redis_conn,
            m: PhantomData::<fn() -> M>,
        };

        let module = M::init(init_context, &params, "").await?;

        Ok(Self {
            redis_conn,
            room_id: SignalingRoomId(room.uuid, breakout_room),
            room,
            participant_id,
            user_id: user.id,
            role,
            control_data: Option::<ControlData>::None,
            module,
            interface,
            rabbitmq_sender,
            events,
            exit: false,
        })
    }

    /// The MockRunners event loop
    async fn run(mut self) {
        let mut rabbitmq_receiver = self.rabbitmq_sender.subscribe();

        while !self.exit {
            let mut ws_messages = vec![];
            let mut rabbitmq_publish = vec![];
            let mut invalidate_data = false;
            let mut events = SelectAll::new();
            let mut exit = None;

            let ctx = ModuleContext {
                timestamp: Timestamp::now(),
                ws_messages: &mut ws_messages,
                rabbitmq_publish: &mut rabbitmq_publish,
                redis_conn: &mut self.redis_conn.clone(),
                invalidate_data: &mut invalidate_data,
                events: &mut events,
                exit: &mut exit,
                m: PhantomData::<fn() -> M>,
            };

            select! {
                res = self.interface.ws.recv() => {
                    let ws_message = res.expect("MockRunners websocket channel is broken");

                    match ws_message {
                        WsMessageIncoming::Module(module_message) =>
                            self.module.on_event(ctx, Event::WsMessage(module_message)).await.expect("Error when handling incoming ws message"),

                        WsMessageIncoming::Control(control_message) =>
                            self.handle_ws_control_message(ctx, control_message).await.expect("Error when handling incoming ws control message"),

                        WsMessageIncoming::CloseWs => {
                            self.exit = true;
                        },
                    }
                    self.handle_module_requested_actions(ws_messages, rabbitmq_publish, invalidate_data, events, exit).await;
                }
                res = rabbitmq_receiver.recv() => {
                    let message = res.expect("Error when receiving on rabbitmq broadcast channel");

                    self.handle_rabbitmq_message(ctx, message).await.expect("Error when handling rabbitmq message");

                    self.handle_module_requested_actions(ws_messages, rabbitmq_publish, invalidate_data, events, exit).await;
                }
                Some((namespace, message)) = self.events.next() => {
                    assert_eq!(namespace, M::NAMESPACE, "Invalid namespace on external event");

                    self.module.on_event(ctx, Event::Ext(*message.downcast().expect("invalid ext type"))).await.expect("Error when handling external event");

                    self.handle_module_requested_actions(ws_messages, rabbitmq_publish, invalidate_data, events, exit).await;
                }
            }
        }

        log::debug!(
            "Shutting down module for participant {}",
            self.participant_id
        );

        self.leave_room().await.expect("Error while leaving room");

        self.destroy().await.expect("Failed to destroy mock runner");
    }

    async fn handle_ws_control_message(
        &mut self,
        mut ctx: ModuleContext<'_, M>,
        control_message: control::incoming::Message,
    ) -> Result<()> {
        match control_message {
            control::incoming::Message::Join(join) => {
                storage::set_attribute(
                    &mut self.redis_conn,
                    self.room_id,
                    self.participant_id,
                    "display_name",
                    &join.display_name,
                )
                .await
                .context("Failed to set display_name")?;

                storage::set_attribute(
                    &mut self.redis_conn,
                    self.room_id,
                    self.participant_id,
                    "joined_at",
                    ctx.timestamp,
                )
                .await
                .context("Failed to set joined timestamp")?;

                storage::set_attribute(
                    &mut self.redis_conn,
                    self.room_id,
                    self.participant_id,
                    "hand_updated_at",
                    ctx.timestamp,
                )
                .await
                .context("Failed to set joined timestamp")?;

                let participant_set =
                    storage::get_all_participants(&mut self.redis_conn, self.room_id)
                        .await
                        .context("Failed to get all active participants")?;

                storage::add_participant_to_set(
                    &mut self.redis_conn,
                    self.room_id,
                    self.participant_id,
                )
                .await
                .context("Failed to add self to participants set")?;

                let mut participants = vec![];

                for id in participant_set {
                    match self.build_participant(id).await {
                        Ok(participant) => participants.push(participant),
                        Err(e) => bail!("Failed to build participant {}, {}", id, e),
                    };
                }

                let mut frontend_data = None;
                let mut participants_data = participants.iter().map(|p| (p.id, None)).collect();
                let mut control_data = ControlData {
                    display_name: join.display_name,
                    hand_is_up: false,
                    joined_at: ctx.timestamp,
                    left_at: None,
                    hand_updated_at: ctx.timestamp,
                };

                self.module
                    .on_event(
                        ctx,
                        Event::Joined {
                            frontend_data: &mut frontend_data,
                            participants: &mut participants_data,
                            control_data: &mut control_data,
                        },
                    )
                    .await?;

                self.control_data = Some(control_data);

                let mut module_data = HashMap::new();

                if let Some(frontend_data) = frontend_data {
                    module_data.insert(
                        M::NAMESPACE,
                        serde_json::to_value(frontend_data)
                            .context("Failed to convert frontend-data to value")?,
                    );
                }

                for participant in participants.iter_mut() {
                    if let Some(data) = participants_data.remove(&participant.id).flatten() {
                        let value = serde_json::to_value(data)
                            .context("Failed to convert module peer frontend data to value")?;

                        participant.module_data.insert(M::NAMESPACE, value);
                    }
                }

                let join_success = control::outgoing::JoinSuccess {
                    id: self.participant_id,
                    role: self.role,
                    module_data,
                    participants,
                };

                self.interface.ws.send(WsMessageOutgoing::Control(
                    outgoing::Message::JoinSuccess(join_success),
                ))?;

                self.publish_rabbitmq_control(control::rabbitmq::Message::Joined(
                    self.participant_id,
                ))?;

                Ok(())
            }
            control::incoming::Message::RaiseHand => {
                storage::set_attribute(
                    &mut self.redis_conn,
                    self.room_id,
                    self.participant_id,
                    "hand_is_up",
                    true,
                )
                .await?;
                storage::set_attribute(
                    &mut self.redis_conn,
                    self.room_id,
                    self.participant_id,
                    "hand_updated_at",
                    ctx.timestamp,
                )
                .await?;

                ctx.invalidate_data();

                self.module.on_event(ctx, Event::RaiseHand).await?;

                Ok(())
            }
            control::incoming::Message::LowerHand => {
                storage::set_attribute(
                    &mut self.redis_conn,
                    self.room_id,
                    self.participant_id,
                    "hand_is_up",
                    false,
                )
                .await?;
                storage::set_attribute(
                    &mut self.redis_conn,
                    self.room_id,
                    self.participant_id,
                    "hand_updated_at",
                    ctx.timestamp,
                )
                .await?;

                ctx.invalidate_data();

                self.module.on_event(ctx, Event::LowerHand).await?;

                Ok(())
            }
        }
    }

    async fn handle_rabbitmq_control_message(
        &mut self,
        ctx: ModuleContext<'_, M>,
        control_message: control::rabbitmq::Message,
    ) -> Result<()> {
        match control_message {
            control::rabbitmq::Message::Joined(participant_id) => {
                if self.participant_id == participant_id {
                    return Ok(());
                }

                let mut participant = self.build_participant(participant_id).await?;

                let mut data = None;

                self.module
                    .on_event(ctx, Event::ParticipantJoined(participant.id, &mut data))
                    .await
                    .context("Module error on ParticipantJoined event")?;

                if let Some(data) = data {
                    let module_data = serde_json::to_value(data).context(
                        "Failed to serialize PeerFrontendData for ParticipantJoined event",
                    )?;

                    participant.module_data.insert(M::NAMESPACE, module_data);
                }

                self.interface.ws.send(WsMessageOutgoing::Control(
                    control::outgoing::Message::Joined(participant),
                ))?;

                Ok(())
            }
            control::rabbitmq::Message::Left(participant_id) => {
                if self.participant_id == participant_id {
                    return Ok(());
                }

                self.module
                    .on_event(ctx, Event::ParticipantLeft(participant_id))
                    .await
                    .context("Module error on ParticipantLeft event")?;

                self.interface.ws.send(WsMessageOutgoing::Control(
                    control::outgoing::Message::Left(control::outgoing::AssociatedParticipant {
                        id: participant_id,
                    }),
                ))?;

                Ok(())
            }
            control::rabbitmq::Message::Update(participant_id) => {
                if self.participant_id == participant_id {
                    return Ok(());
                }

                let mut participant = self.build_participant(participant_id).await?;

                let mut data = None;

                self.module
                    .on_event(ctx, Event::ParticipantUpdated(participant.id, &mut data))
                    .await
                    .context("Module error on ParticipantUpdated event")?;

                if let Some(data) = data {
                    let module_data = serde_json::to_value(data).context(
                        "Failed to serialize PeerFrontendData for ParticipantUpdated event",
                    )?;

                    participant.module_data.insert(M::NAMESPACE, module_data);
                }

                self.interface.ws.send(WsMessageOutgoing::Control(
                    control::outgoing::Message::Update(participant),
                ))?;

                Ok(())
            }
        }
    }

    fn publish_rabbitmq_control(&mut self, message: control::rabbitmq::Message) -> Result<()> {
        let message = serde_json::to_string(&Namespaced {
            namespace: NAMESPACE,
            payload: message,
        })?;

        let rabbitmq_publish = RabbitMqPublish {
            exchange: control::rabbitmq::room_exchange_name(self.room.uuid),
            routing_key: control::rabbitmq::room_all_routing_key().into(),
            message,
        };

        self.rabbitmq_sender
            .send(rabbitmq_publish)
            .map_err(|e| anyhow::Error::msg(format!("Unable to send rabbbitmq_publish, {}", e)))?;
        Ok(())
    }

    /// Check if the routing key matches this participant and serialize the rabbitmq message
    async fn handle_rabbitmq_message(
        &mut self,
        ctx: ModuleContext<'_, M>,
        rabbitmq_publish: RabbitMqPublish,
    ) -> Result<()> {
        let participant_routing_key =
            control::rabbitmq::room_participant_routing_key(self.participant_id);

        let user_routing_key = control::rabbitmq::room_user_routing_key(self.user_id);

        if !(rabbitmq_publish.routing_key == "participant.all"
            || rabbitmq_publish.routing_key == participant_routing_key
            || rabbitmq_publish.routing_key == user_routing_key)
        {
            return Ok(());
        }

        let namespaced = serde_json::from_str::<Namespaced<Value>>(&rabbitmq_publish.message)
            .context("Failed to read incoming rabbitmq message")?;

        if namespaced.namespace == NAMESPACE {
            let control_message = serde_json::from_value(namespaced.payload)?;

            self.handle_rabbitmq_control_message(ctx, control_message)
                .await
                .context("Error when handling ws control message")?;

            Ok(())
        } else if namespaced.namespace == M::NAMESPACE {
            let module_message = serde_json::from_value(namespaced.payload)?;

            self.module
                .on_event(ctx, Event::RabbitMq(module_message))
                .await
                .context("Module error on rabbitmq event")?;

            Ok(())
        } else {
            bail!(
                "Got rabbitmq message with unknown namespace '{}'",
                namespaced.namespace
            )
        }
    }

    async fn handle_module_requested_actions(
        &mut self,
        ws_messages: Vec<NamespacedOutgoing<'_, M::Outgoing>>,
        rabbitmq_publish: Vec<RabbitMqPublish>,
        invalidate_data: bool,
        events: SelectAll<AnyStream>,
        exit: Option<CloseCode>,
    ) {
        for ws_message in ws_messages {
            self.interface
                .ws
                .send(WsMessageOutgoing::Module(ws_message.payload))
                .expect("Error sending outgoing module message");
        }

        for rabbitmq_message in rabbitmq_publish {
            self.rabbitmq_sender
                .send(rabbitmq_message)
                .expect("Error sending outgoing module message");
        }

        if invalidate_data {
            self.publish_rabbitmq_control(control::rabbitmq::Message::Update(self.participant_id))
                .expect("Error sending rabbitmq update message");
        }

        for event in events {
            self.events.push(event)
        }

        if let Some(exit) = exit {
            self.exit = true;

            log::debug!("Module requested exit with CloseCode: {}", exit);
        }
    }

    async fn leave_room(&mut self) -> Result<()> {
        let mut ws_messages = vec![];
        let mut rabbitmq_publish = vec![];
        let mut invalidate_data = false;
        let mut events = SelectAll::new();
        let mut exit = None;

        let ctx = ModuleContext {
            timestamp: Timestamp::now(),
            ws_messages: &mut ws_messages,
            rabbitmq_publish: &mut rabbitmq_publish,
            redis_conn: &mut self.redis_conn,
            invalidate_data: &mut invalidate_data,
            events: &mut events,
            exit: &mut exit,
            m: PhantomData::<fn() -> M>,
        };

        self.module
            .on_event(ctx, Event::Leaving)
            .await
            .context("Module error on Leaving event")?;

        self.handle_module_requested_actions(
            ws_messages,
            rabbitmq_publish,
            invalidate_data,
            events,
            exit,
        )
        .await;

        Ok(())
    }

    async fn build_participant(&mut self, id: ParticipantId) -> Result<outgoing::Participant> {
        let mut participant = outgoing::Participant {
            id,
            module_data: Default::default(),
        };

        let display_name: String =
            storage::get_attribute(&mut self.redis_conn, self.room_id, id, "display_name").await?;
        let joined_at: Timestamp =
            storage::get_attribute(&mut self.redis_conn, self.room_id, id, "joined_at").await?;

        let hand_is_up: bool =
            storage::get_attribute(&mut self.redis_conn, self.room_id, id, "hand_is_up").await?;
        let hand_updated_at: Timestamp =
            storage::get_attribute(&mut self.redis_conn, self.room_id, id, "hand_updated_at")
                .await?;

        participant.module_data.insert(
            NAMESPACE,
            serde_json::to_value(ControlData {
                display_name,
                hand_is_up,
                joined_at,
                hand_updated_at,
                left_at: None,
            })
            .expect("Failed to convert ControlData to serde_json::Value"),
        );

        Ok(participant)
    }

    async fn destroy(mut self) -> Result<()> {
        let mut set_lock = storage::room_mutex(self.room_id);

        let set_guard = set_lock.lock(&mut self.redis_conn).await?;

        // Remove participant from set and check if set is empty
        let remaining = storage::remove_participant_from_set(
            &set_guard,
            &mut self.redis_conn,
            self.room_id,
            self.participant_id,
        )
        .await?;

        let destroy_room = remaining == 0;

        self.publish_rabbitmq_control(control::rabbitmq::Message::Left(self.participant_id))
            .context("Failed to send rabbitmq left message on destroy")?;

        let ctx = DestroyContext {
            redis_conn: &mut self.redis_conn.clone(),
            destroy_room,
        };
        let module = self.module;

        module.on_destroy(ctx).await;

        if destroy_room {
            storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "display_name")
                .await?;

            storage::remove_attribute_key(&mut self.redis_conn, self.room_id, "user_id").await?;
        }

        set_guard
            .unlock(&mut self.redis_conn)
            .await
            .context("Failed to unlock set_guard r3dlock while destroying mockrunner")
    }
}

/// Represents a WebSocket message sent from the Client to the Module
enum WsMessageIncoming<M>
where
    M: SignalingModule,
{
    Module(M::Incoming),
    Control(control::incoming::Message),
    /// The 'WebSocket' was closed
    CloseWs,
}

/// Represents a WebSocket message sent from the Module to the Client
pub enum WsMessageOutgoing<M>
where
    M: SignalingModule,
{
    Module(M::Outgoing),
    Control(control::outgoing::Message),
}

impl<M> std::fmt::Debug for WsMessageOutgoing<M>
where
    M: SignalingModule,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Module(arg0) => f.debug_tuple("Module").field(arg0).finish(),
            Self::Control(arg0) => f.debug_tuple("Control").field(arg0).finish(),
        }
    }
}

impl<M> PartialEq for WsMessageOutgoing<M>
where
    M: SignalingModule,
{
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Module(l0), Self::Module(r0)) => l0 == r0,
            (Self::Control(l0), Self::Control(r0)) => l0 == r0,
            _ => false,
        }
    }
}

/// A interface used by the runner to interact with the client([`ModuleTester`])
struct ClientInterface<M>
where
    M: SignalingModule,
{
    ws: Interface<WsMessageOutgoing<M>, WsMessageIncoming<M>>,
}

/// A interface used by the client to interact with the runner([`MockRunner`])
struct RunnerInterface<M>
where
    M: SignalingModule,
{
    ws: Interface<WsMessageIncoming<M>, WsMessageOutgoing<M>>,
}

struct Interface<S, R> {
    sender: UnboundedSender<S>,
    receiver: UnboundedReceiver<R>,
}

impl<S, R> Interface<S, R> {
    fn new(sender: UnboundedSender<S>, receiver: UnboundedReceiver<R>) -> Self {
        Self { sender, receiver }
    }

    fn send(&self, value: S) -> Result<()> {
        self.sender
            .send(value)
            .map_err(|e| anyhow::Error::msg(format!("MockWs failed to send message, {}", e)))
    }

    async fn recv(&mut self) -> Option<R> {
        self.receiver.recv().await
    }
}

/// Creates two interfaces that complement each other for bidirectional communication
///
/// eg.:
/// ``` text
/// Interface1 sending A and receiving B
/// Interface2 sending B and receiving A
/// ```
fn create_interface<A, B>() -> (Interface<A, B>, Interface<B, A>) {
    let (sender_a, receiver_a) = mpsc::unbounded_channel();
    let (sender_b, receiver_b) = mpsc::unbounded_channel();

    (
        Interface::new(sender_a, receiver_b),
        Interface::new(sender_b, receiver_a),
    )
}

/// Create the interfaces for the Client and Runner
async fn create_interfaces<M>() -> (ClientInterface<M>, RunnerInterface<M>)
where
    M: SignalingModule,
{
    let (ws_client_interface, ws_runner_interface) =
        create_interface::<WsMessageOutgoing<M>, WsMessageIncoming<M>>();

    let client_interface = ClientInterface {
        ws: ws_client_interface,
    };

    let runner_interface = RunnerInterface {
        ws: ws_runner_interface,
    };

    (client_interface, runner_interface)
}
