use crate::api::signaling::ParticipantId;
use adapter::ActixTungsteniteAdapter;
use anyhow::Result;
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
use tokio_stream::Stream;
use uuid::Uuid;

mod adapter;
mod echo;
mod http;
mod modules;
mod runner;

pub use echo::Echo;
pub use http::SignalingHttpModule;

type WebSocket = WebSocketStream<ActixTungsteniteAdapter>;

/// Event passed to [`WebSocketModule::on_event`]
pub enum Event<'evt, M>
where
    M: SignalingModule,
{
    /// The participant joined the room
    Joined {
        /// The module can set this option to Some(M::FrontendData) to populate
        /// the `join_success` message with additional information to the frontend module counterpart
        frontend_data: &'evt mut Option<M::FrontendData>,

        /// List of participants already inside the room.
        ///
        /// The module can populate participant specific frontend-data, which is sent inside
        /// the participant inside the `join_success` message
        participants: &'evt mut HashMap<ParticipantId, Option<M::PeerFrontendData>>,
    },

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

    /// External event provided by [`WebSocketModule::ExtEventStream`]
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
    room: Uuid,
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

    /// ID of the room the participant is inside
    pub fn room_id(&self) -> Uuid {
        self.room
    }

    /// Access to a redis connection
    pub fn redis_conn(&mut self) -> &mut ConnectionManager {
        &mut self.redis_conn
    }

    /// Add a rabbitmq exchange to be created
    // TODO Not used yet anywhere
    #[allow(dead_code)]
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
    // TODO Used in ee-chat, remove `allow` then
    #[allow(dead_code)]
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
    invalidate_data: &'ctx mut bool,
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

    /// Signals that the data related to the participant has changed
    pub fn invalidate_data(&mut self) {
        *self.invalidate_data = true;
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

    /// The module params, can be any type that is `Send` + `Sync`
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
    ///
    /// Calls to [`WsCtx::ws_send`] are discarded
    async fn init(
        ctx: InitContext<'_, Self>,
        params: &Self::Params,
        protocol: &'static str,
    ) -> Result<Self>;

    /// Events related to this module will be passed into this function together with [EventCtx]
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
