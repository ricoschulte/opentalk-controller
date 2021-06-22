use crate::api::signaling::storage::Storage;
use crate::api::signaling::ParticipantId;
use adapter::ActixTungsteniteAdapter;
use anyhow::Result;
use async_tungstenite::tungstenite::Message;
use async_tungstenite::WebSocketStream;
use futures::stream::SelectAll;
use modules::{any_stream, AnyStream};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use tokio_stream::Stream;

mod adapter;
mod echo;
mod http;
mod modules;
mod runner;

pub use echo::Echo;
pub use http::SignalingHttpModule;

type WebSocket = WebSocketStream<ActixTungsteniteAdapter>;

/// Event passed to [`WebSocketModule::on_event`]
pub enum Event<M>
where
    M: SignalingModule,
{
    /// Participant with the associated id has joined the room
    ParticipantJoined(ParticipantId, M::PeerFrontendData),

    /// Participant with the associated id has left the room
    ParticipantLeft(ParticipantId),

    /// Participant data has changed
    ParticipantUpdated(ParticipantId, M::PeerFrontendData),

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

/// Context passed to the module
///
/// Can be used to send websocket messages
pub struct ModuleContext<'ctx, M>
where
    M: SignalingModule,
{
    id: ParticipantId,
    ws_messages: &'ctx mut Vec<Message>,
    rabbitmq_messages: &'ctx mut Vec<(Option<ParticipantId>, String)>,
    events: &'ctx mut SelectAll<AnyStream>,
    storage: &'ctx mut Storage,
    invalidate_data: &'ctx mut bool,
    m: PhantomData<fn() -> M>,
}

impl<M> ModuleContext<'_, M>
where
    M: SignalingModule,
{
    pub fn participant_id(&self) -> ParticipantId {
        self.id
    }

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
    pub fn rabbitmq_send(&mut self, target: Option<ParticipantId>, message: M::RabbitMqMessage) {
        self.rabbitmq_messages.push((
            target,
            Namespaced {
                namespace: M::NAMESPACE,
                payload: message,
            }
            .to_json(),
        ));
    }

    /// Access to local state of the websocket
    pub fn storage(&mut self) -> &mut Storage {
        self.storage
    }

    /// Signals that the data related to the participant has changed
    pub fn invalidate_data(&mut self) {
        *self.invalidate_data = true;
    }

    pub fn add_event_stream<S>(&mut self, stream: S)
    where
        S: Stream<Item = M::ExtEvent> + 'static,
    {
        self.events.push(any_stream(M::NAMESPACE, stream));
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
    // TODO Remove this deserialize bound:
    // When a peer participant joins the runner asks all modules to collect data into a `Participant`.
    // But since the module doesnt know if the data collected was because of an update or join or something else.
    //
    // The runner dispatches a ParticipantJoined event which needs to contain the module data,
    // which was collected before into `Participant` as serde_json::Value. Therefore the value will
    // be deserialized back into PeerFrontendData.
    //
    // This is a hack and should be removed.
    type PeerFrontendData: for<'de> Deserialize<'de> + Serialize;

    /// Constructor of the module
    ///
    /// Provided with the websocket context the modules params and the negotiated protocol
    ///
    /// Calls to [`WsCtx::ws_send`] are discarded
    async fn init(
        ctx: ModuleContext<'_, Self>,
        params: &Self::Params,
        protocol: &'static str,
    ) -> Result<Self>;

    /// Events related to this module will be passed into this function together with [EventCtx]
    /// which gives access to the websocket and other related information.
    async fn on_event(&mut self, ctx: ModuleContext<'_, Self>, event: Event<Self>) -> Result<()>;

    async fn get_frontend_data(&self) -> Self::FrontendData;
    async fn get_frontend_data_for(
        &self,
        storage: &mut Storage,
        participant: ParticipantId,
    ) -> Result<Self::PeerFrontendData>;

    /// Before dropping the module this function will be called
    async fn on_destroy(self, storage: &mut Storage);
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
