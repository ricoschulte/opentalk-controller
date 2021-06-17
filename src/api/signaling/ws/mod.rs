use crate::api::signaling::storage::Storage;
use crate::api::signaling::ParticipantId;
use crate::api::v1::middleware::oidc_auth::check_access_token;
use crate::db::DbInterface;
use crate::modules::http::HttpModule;
use crate::oidc::OidcContext;
use actix_web::http::{header, HeaderValue};
use actix_web::web::{Data, ServiceConfig};
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use adapter::ActixTungsteniteAdapter;
use anyhow::Result;
use async_tungstenite::tungstenite::protocol::Role;
use async_tungstenite::tungstenite::Message;
use async_tungstenite::WebSocketStream;
use futures::stream::SelectAll;
use modules::{any_stream, AnyStream, ModuleBuilder, ModuleBuilderImpl, Modules};
use openidconnect::AccessToken;
use redis::aio::MultiplexedConnection;
use runner::Runner;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::rc::Rc;
use tokio::task;
use tokio_stream::Stream;

use uuid::Uuid;
mod adapter;
mod echo;
mod modules;
mod runner;

pub use echo::Echo;

type WebSocket = WebSocketStream<ActixTungsteniteAdapter>;

/// Event passed to [`WebSocketModule::on_event`].
pub enum Event<M>
where
    M: SignalingModule,
{
    /// Participant with the associated id has joined the room
    ParticipantJoined(ParticipantId),

    /// Participant with the associated id has left the room
    ParticipantLeft(ParticipantId),

    /// Participant data has changed
    ParticipantUpdated(ParticipantId),

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
    type PeerFrontendData: Serialize;

    /// Constructor of the module
    ///
    /// Provided with the websocket context the modules params and the negotiated protocol
    ///
    /// Calls to [`WsCtx::ws_send`] are discarded
    async fn init(
        ctx: ModuleContext<'_, Self>,
        params: &Self::Params,
        protocol: &'static str,
    ) -> Self;

    /// Events related to this module will be passed into this function together with [EventCtx]
    /// which gives access to the websocket and other related information.
    async fn on_event(&mut self, ctx: ModuleContext<'_, Self>, event: Event<Self>);

    async fn get_frontend_data(&self) -> Self::FrontendData;
    async fn get_frontend_data_for(
        &self,
        storage: &mut Storage,
        participant: ParticipantId,
    ) -> Result<Self::PeerFrontendData>;

    /// Before dropping the module this function will be called
    async fn on_destroy(self, storage: &mut Storage);
}

/// Signaling endpoint which can be registered to a `ApplicationBuilder`.
///
/// Logic is implemented via modules.
/// See [`SignalingModule`] and [`SignalingHttpModule::add_module`]
pub struct SignalingHttpModule {
    protocols: &'static [&'static str],
    modules: Vec<Box<dyn ModuleBuilder>>,

    redis_conn: MultiplexedConnection,
    rabbit_mq_channel: lapin::Channel,
}

impl SignalingHttpModule {
    pub fn new(redis_conn: MultiplexedConnection, rabbit_mq_channel: lapin::Channel) -> Self {
        Self {
            protocols: &["k3k-signaling-json-v1"],
            modules: Default::default(),
            redis_conn,
            rabbit_mq_channel,
        }
    }

    /// Add a implementation of [`SignalingModule`] together with some options used to initialize
    /// the module.
    ///
    /// The module itself will be created on successful websocket connection.
    pub fn add_module<M>(&mut self, params: M::Params)
    where
        M: SignalingModule + 'static,
    {
        self.modules.push(Box::new(ModuleBuilderImpl {
            m: PhantomData::<fn() -> M>,
            params,
        }));
    }
}

impl HttpModule for SignalingHttpModule {
    fn register(&self, app: &mut ServiceConfig) {
        let protocols = self.protocols;
        let modules = Rc::from(self.modules.clone().into_boxed_slice());
        let redis_conn = self.redis_conn.clone();
        let rabbit_mq_channel = self.rabbit_mq_channel.clone();

        app.service(web::scope("").route(
            "/signaling",
            web::get().to(move |db_ctx, oidc_ctx, req, stream| {
                ws_service(
                    db_ctx,
                    oidc_ctx,
                    redis_conn.clone(),
                    rabbit_mq_channel.clone(),
                    protocols,
                    Rc::clone(&modules),
                    req,
                    stream,
                )
            }),
        ));
    }
}

#[allow(clippy::too_many_arguments)]
async fn ws_service(
    db_ctx: Data<DbInterface>,
    oidc_ctx: Data<OidcContext>,
    redis_conn: MultiplexedConnection,
    rabbit_mq_channel: lapin::Channel,
    protocols: &'static [&'static str],
    modules: Rc<[Box<dyn ModuleBuilder>]>,
    req: HttpRequest,
    stream: web::Payload,
) -> actix_web::Result<HttpResponse> {
    let mut protocol = None;
    let mut access_token = None;

    if let Some(value) = req
        .headers()
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|v| v.to_str().ok())
    {
        let mut values = value.split(',').map(str::trim);

        while let Some(value) = values.next() {
            if value == "access_token" {
                access_token = values.next();
                continue;
            } else if protocol.is_some() {
                continue;
            } else if let Some(value) = protocols.iter().find(|&&v| v == value) {
                protocol = Some(value);
            }
        }
    }

    let (protocol, access_token) = match (protocol, access_token) {
        (Some(protocol), Some(access_token)) => (protocol, access_token),
        // TODO how to handle the rejection properly
        // usually if no protocol offered by the client is compatible, the server should continue
        // with the websocket upgrade and return an empty SEC_WEBSOCKET_PROTOCOL header, to let the
        // client decide if they want to continue or not. (chromium aborts and firefox continues)
        //
        // There is no defined way how to handle authorization for websockets other than
        // sending a challenge for the client to solve. That makes it hard to decide which status
        // code should be returned here if the access_token is missing or invalid.
        //
        // Possible solution:
        // Get short lived one time ticket token from REST endpoint
        // Access endpoint with ?ticket=... query
        // This avoids having the access_token inside server logs and keeps it outside headers
        // it doesnt belong into
        _ => return Ok(HttpResponse::Forbidden().finish()),
    };

    let user = check_access_token(db_ctx, oidc_ctx, AccessToken::new(access_token.into())).await?;

    let mut response = ws::handshake(&req)?;

    let (adapter, actix_stream) = ActixTungsteniteAdapter::from_actix_payload(stream);
    let websocket = WebSocketStream::from_raw_socket(adapter, Role::Server, None).await;

    // TODO: Get these information from somewhere reliable (ticket-system + redis)?
    let id = ParticipantId::new();
    let room_id = Uuid::nil();

    // Create redis storage for this ws-task
    let mut storage = Storage::new(redis_conn, room_id);

    // Build and initialize the modules
    let mut builder = Modules::default();
    for module in modules.iter() {
        module.build(id, &mut builder, &mut storage, protocol).await;
    }

    let runner = match Runner::init(
        id,
        room_id,
        user,
        builder,
        storage,
        rabbit_mq_channel,
        websocket,
    )
    .await
    {
        Ok(runner) => runner,
        Err(e) => {
            log::error!("Failed to initialize runner, {}", e);
            return HttpResponse::InternalServerError().await;
        }
    };

    // Spawn the runner task
    task::spawn_local(runner.run());

    // TODO: maybe change the SEC_WEBSOCKET_PROTOCOL header to access_token=kdpaosd2eja9dj,k3k-signaling-json-v1 to avoid ordering
    response.insert_header((
        header::SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_str(protocol).unwrap(),
    ));

    Ok(response.streaming(actix_stream))
}

#[derive(Deserialize, Serialize)]
pub struct Namespaced<'n, O> {
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
