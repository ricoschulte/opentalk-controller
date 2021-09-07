use crate::api::signaling::ws_modules::breakout::BreakoutRoomId;
use crate::api::signaling::ws_modules::control::ControlData;
use crate::api::signaling::{ParticipantId, Role, SignalingRoomId};
use crate::db::rooms::Room;
use crate::db::users::User;
use crate::db::DbInterface;
use adapter::ActixTungsteniteAdapter;
use anyhow::Result;
use async_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use async_tungstenite::tungstenite::Message;
use async_tungstenite::WebSocketStream;
use futures::stream::SelectAll;
use lapin::options::{ExchangeDeclareOptions, QueueBindOptions};
use lapin::ExchangeKind;
use modules::{any_stream, AnyStream};
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio_stream::Stream;

mod adapter;
mod echo;
mod http;
mod modules;
mod runner;

pub use echo::Echo;
pub use http::ws_service;
pub use http::SignalingModules;
pub use http::SignalingProtocols;

type WebSocket = WebSocketStream<ActixTungsteniteAdapter>;

/// Event passed to [`SignalingModule::on_event`]
pub enum Event<'evt, M>
where
    M: SignalingModule,
{
    /// The participant joined the room
    Joined {
        /// Data set by the control module. Some modules require attributes specified by the
        /// control module which are provided here on join
        control_data: &'evt ControlData,

        /// The module can set this option to Some(M::FrontendData) to populate
        /// the `join_success` message with additional information to the frontend module counterpart
        frontend_data: &'evt mut Option<M::FrontendData>,

        /// List of participants already inside the room.
        ///
        /// The module can populate participant specific frontend-data, which is sent inside
        /// the participant inside the `join_success` message
        participants: &'evt mut HashMap<ParticipantId, Option<M::PeerFrontendData>>,
    },

    /// The participant is in the process of leaving the room, this event will be called before
    /// `on_destroy` is called and before the rabbitmq control message `Left` has been sent.
    ///
    /// Note: Calls to `ModuleContext::ws_send` when receiving this event will almost certainly fail
    Leaving,

    /// A user can request attention by 'raising' his hand, this event gets broadcast to every
    /// module.
    RaiseHand,

    /// User lowered his hand and no longer requests attention.
    LowerHand,

    /// Participant with the associated id has joined the room
    ParticipantJoined(ParticipantId, &'evt mut Option<M::PeerFrontendData>),

    /// Participant with the associated id has left the room
    ParticipantLeft(ParticipantId),

    /// Participant data has changed, an options to `M::PeerFrontendData`
    ParticipantUpdated(ParticipantId, &'evt mut Option<M::PeerFrontendData>),

    /// Received websocket message
    WsMessage(M::Incoming),

    /// RabbitMQ queue received a message for this module
    RabbitMq(M::RabbitMqMessage),

    /// External event provided by eventstream which was added using [`InitContext::add_event_stream`].
    ///
    /// Modules that didnt register external events will
    /// never receive this variant and can ignore it.
    Ext(M::ExtEvent),
}

/// Context passed to the `init` function
pub struct InitContext<'ctx, M>
where
    M: SignalingModule,
{
    id: ParticipantId,
    room: &'ctx Room,
    breakout_room: Option<BreakoutRoomId>,
    user: &'ctx User,
    role: Role,
    db: &'ctx Arc<DbInterface>,
    rabbitmq_exchanges: &'ctx mut Vec<RabbitMqExchange>,
    rabbitmq_bindings: &'ctx mut Vec<RabbitMqBinding>,
    events: &'ctx mut SelectAll<AnyStream>,
    redis_conn: &'ctx mut ConnectionManager,
    m: PhantomData<fn() -> M>,
}

struct RabbitMqExchange {
    name: String,
    kind: ExchangeKind,
    options: ExchangeDeclareOptions,
}

struct RabbitMqBinding {
    routing_key: String,
    exchange: String,
    options: QueueBindOptions,
}

impl<M> InitContext<'_, M>
where
    M: SignalingModule,
{
    /// ID of the participant the module instance belongs to
    pub fn participant_id(&self) -> ParticipantId {
        self.id
    }

    /// Returns a reference to the database representation of the the room
    ///
    /// Note that the room will always be the same regardless if inside a
    /// breakout room or not.
    pub fn room(&self) -> &Room {
        self.room
    }

    /// ID of the room currently inside, this MUST be used when a module does not care about
    /// whether it is inside a breakout room or not.
    pub fn room_id(&self) -> SignalingRoomId {
        SignalingRoomId(self.room.uuid, self.breakout_room)
    }

    /// Returns the ID of the breakout room, if inside one
    pub fn breakout_room(&self) -> Option<BreakoutRoomId> {
        self.breakout_room
    }

    /// Returns the user associated with the participant
    pub fn user(&self) -> &User {
        self.user
    }

    /// Returns the role of participant inside the room
    pub fn role(&self) -> Role {
        self.role
    }

    /// Returns a reference to the controllers database interface
    pub fn db(&self) -> &Arc<DbInterface> {
        self.db
    }

    /// Access to a redis connection
    pub fn redis_conn(&mut self) -> &mut ConnectionManager {
        &mut self.redis_conn
    }

    /// Add a rabbitmq exchange to be created
    pub fn add_rabbitmq_exchange(
        &mut self,
        name: String,
        kind: ExchangeKind,
        options: ExchangeDeclareOptions,
    ) {
        self.rabbitmq_exchanges.push(RabbitMqExchange {
            name,
            kind,
            options,
        });
    }

    /// Add a rabbitmq binding to bind the queue to
    pub fn add_rabbitmq_binding(
        &mut self,
        routing_key: String,
        exchange: String,
        options: QueueBindOptions,
    ) {
        self.rabbitmq_bindings.push(RabbitMqBinding {
            routing_key,
            exchange,
            options,
        });
    }

    /// Add a custom event stream which return `M::ExtEvent`
    pub fn add_event_stream<S>(&mut self, stream: S)
    where
        S: Stream<Item = M::ExtEvent> + 'static,
    {
        self.events.push(any_stream(M::NAMESPACE, stream));
    }
}

/// Context passed to the module
///
/// Can be used to send websocket messages
pub struct ModuleContext<'ctx, M>
where
    M: SignalingModule,
{
    ws_messages: &'ctx mut Vec<Message>,
    rabbitmq_publish: &'ctx mut Vec<RabbitMqPublish>,
    redis_conn: &'ctx mut ConnectionManager,
    events: &'ctx mut SelectAll<AnyStream>,
    invalidate_data: &'ctx mut bool,
    exit: &'ctx mut Option<CloseCode>,
    m: PhantomData<fn() -> M>,
}

struct RabbitMqPublish {
    exchange: String,
    routing_key: String,
    message: String,
}

impl<M> ModuleContext<'_, M>
where
    M: SignalingModule,
{
    /// Queue a outgoing message to be sent via the websocket
    /// after exiting the `on_event` function
    ///
    /// # Panics
    ///
    /// If `M::Outgoing` type is not a json map or object it cannot be flattened causing a panic.
    pub fn ws_send(&mut self, message: M::Outgoing) {
        self.ws_messages.push(Message::Text(
            Namespaced {
                namespace: M::NAMESPACE,
                payload: message,
            }
            .to_json(),
        ));
    }

    /// Queue a outgoing message to be sent via rabbitmq
    pub fn rabbitmq_publish(
        &mut self,
        exchange: String,
        routing_key: String,
        message: M::RabbitMqMessage,
    ) {
        self.rabbitmq_publish.push(RabbitMqPublish {
            exchange,
            routing_key,
            message: Namespaced {
                namespace: M::NAMESPACE,
                payload: message,
            }
            .to_json(),
        });
    }

    /// Access to the storage of the room
    pub fn redis_conn(&mut self) -> &mut ConnectionManager {
        &mut self.redis_conn
    }

    /// Add a custom event stream which return `M::ExtEvent`
    pub fn add_event_stream<S>(&mut self, stream: S)
    where
        S: Stream<Item = M::ExtEvent> + 'static,
    {
        self.events.push(any_stream(M::NAMESPACE, stream));
    }

    /// Signals that the data related to the participant has changed
    pub fn invalidate_data(&mut self) {
        *self.invalidate_data = true;
    }

    pub fn exit(&mut self, code: Option<CloseCode>) {
        *self.exit = Some(code.unwrap_or(CloseCode::Normal));
    }
}

/// Context passed to the `destroy` function
pub struct DestroyContext<'ctx> {
    redis_conn: &'ctx mut ConnectionManager,
    destroy_room: bool,
}

impl DestroyContext<'_> {
    /// Access to a redis connection
    pub fn redis_conn(&mut self) -> &mut ConnectionManager {
        &mut self.redis_conn
    }

    /// Returns true if the module belongs to the last participant inside a room
    pub fn destroy_room(&self) -> bool {
        self.destroy_room
    }
}

/// Extension to a the signaling websocket
#[async_trait::async_trait(?Send)]
pub trait SignalingModule: Sized + 'static {
    /// Defines the websocket message namespace
    ///
    /// Must be unique between all registered modules.
    const NAMESPACE: &'static str;

    /// The module params, can be any type that is `Clone` + `Send` + `Sync`
    ///
    /// Will get passed to `init` as parameter
    type Params: Clone + Send + Sync;

    /// The websocket incoming message type
    type Incoming: for<'de> Deserialize<'de>;

    /// The websocket outgoing message type
    type Outgoing: Serialize;

    /// Message type sent over rabbitmq to other participant's modules
    type RabbitMqMessage: for<'de> Deserialize<'de> + Serialize;

    /// Optional event type, yielded by `ExtEventStream`
    ///
    /// If the module does not register external events it should be set to `()`.
    type ExtEvent;

    /// Data about the owning user of the ws-module which is sent to the frontend on join
    type FrontendData: Serialize;

    /// Data about a peer which is sent to the frontend
    type PeerFrontendData: Serialize;

    /// Constructor of the module
    ///
    /// Provided with the websocket context the modules params and the negotiated protocol
    async fn init(
        ctx: InitContext<'_, Self>,
        params: &Self::Params,
        protocol: &'static str,
    ) -> Result<Self>;

    /// Events related to this module will be passed into this function together with [`ModuleContext`]
    /// which gives access to the websocket and other related information.
    async fn on_event(
        &mut self,
        ctx: ModuleContext<'_, Self>,
        event: Event<'_, Self>,
    ) -> Result<()>;

    /// Before dropping the module this function will be called
    async fn on_destroy(self, ctx: DestroyContext<'_>);
}

/// The root of all websocket messages
#[derive(Deserialize, Serialize)]
pub(super) struct Namespaced<'n, O> {
    pub namespace: &'n str,
    pub payload: O,
}

impl<'n, O> Namespaced<'n, O>
where
    O: Serialize,
{
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Failed to convert namespaced to json")
    }
}
