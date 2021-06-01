use super::HttpModule;
use actix_web::dev::Extensions;
use actix_web::http::{header, HeaderValue};
use actix_web::web::ServiceConfig;
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use adapter::ActixTungsteniteAdapter;
use async_tungstenite::tungstenite::protocol::Role;
use async_tungstenite::tungstenite::Message;
use async_tungstenite::WebSocketStream;
use futures::Stream;
use modules::Modules;
use modules::{ModuleBuilder, ModuleBuilderImpl};
use runner::{Namespaced, Runner};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;
use tokio::task;

mod adapter;
mod echo;
mod modules;
mod runner;

pub use echo::Echo;

type WebSocket = WebSocketStream<ActixTungsteniteAdapter>;

/// Event passed to [`WebSocketModule::on_event`].
pub enum Event<M>
where
    M: WebSocketModule,
{
    /// Received websocket message
    WsMessage(M::Incoming),

    /// External event provided by [`WebSocketModule::ExtEventStream`]
    ///
    /// Modules that didnt register external events will
    /// never receive this variant and can ignore it.
    Ext(M::ExtEvent),
}

/// Context of an Event
///
/// Can be used to send websocket messages
pub struct EventCtx<'ctx, M>
where
    M: WebSocketModule,
{
    ws_messages: &'ctx mut Vec<Message>,
    local_state: &'ctx mut Extensions,
    m: PhantomData<fn() -> M>,
}

impl<M> EventCtx<'_, M>
where
    M: WebSocketModule,
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

    /// Access to local state of the websocket
    pub fn local_state(&mut self) -> &mut Extensions {
        self.local_state
    }
}

/// Extension to a websocket
#[async_trait::async_trait(?Send)]
pub trait WebSocketModule: Sized + 'static {
    /// Defines the websocket message namespace
    ///
    /// Must be unique between all registered modules.
    const NAMESPACE: &'static str;

    /// The module options, can be any type that is `Send` + `Sync`
    ///
    /// Will get passed to `init` as parameter
    type Params: Send + Sync;

    /// The websocket incoming message type
    type Incoming: for<'de> Deserialize<'de>;

    /// The websocket outgoing message type
    type Outgoing: Serialize;

    /// Optional event type, yielded by `ExtEventStream`
    ///
    /// If the module does not register external events it should be set to `()`.
    type ExtEvent: Any + 'static;

    /// Optional event stream which yields `ExtEvent`
    ///
    /// If the module does not register external events it can be set to `tokio_stream::Pending<()>`
    type ExtEventStream: Stream<Item = Self::ExtEvent>;

    /// Constructor of the module.
    ///
    /// Provided with its initial data and the negotiated websocket protocol
    async fn init(options: &Self::Params, protocol: &'static str) -> Self;

    /// Getter function which, if any external events are registered, will return a stream yielding
    /// external events.
    ///
    /// This will be called only once after initialization of the module.
    ///
    /// If the module does not register external events it should just return `None`
    async fn events(&mut self) -> Option<Self::ExtEventStream>;

    /// Events related to this module will be passed into this function together with [EventCtx]
    /// which gives access to the websocket and other related information.
    async fn on_event(&mut self, ctx: EventCtx<'_, Self>, event: Event<Self>);

    /// Before dropping the module this function will be called
    async fn on_destroy(self);
}

/// Websocket endpoint which can be registered to a `ApplicationBuilder`.
///
/// Logic is implemented via modules.
/// See [`WebSocketModule`] and [`WebSocketHttpModule::add_module`]
pub struct WebSocketHttpModule {
    path: &'static str,
    protocols: &'static [&'static str],

    modules: Vec<Box<dyn ModuleBuilder>>,
}

impl WebSocketHttpModule {
    pub fn new(path: &'static str, protocols: &'static [&'static str]) -> Self {
        Self {
            path,
            protocols,
            modules: Default::default(),
        }
    }

    /// Add a implementation of [`WebSocketModule`] together with some options used to initialize
    /// the module.
    ///
    /// The module itself will be created on successful websocket connection.
    pub fn add_module<M>(&mut self, params: M::Params)
    where
        M: WebSocketModule + 'static,
    {
        self.modules.push(Box::new(ModuleBuilderImpl {
            m: PhantomData::<fn() -> M>,
            params: Arc::new(params),
        }));
    }
}

impl HttpModule for WebSocketHttpModule {
    fn register(&self, app: &mut ServiceConfig) {
        let protocols = self.protocols;
        let modules = Rc::from(self.modules.clone().into_boxed_slice());

        app.service(web::scope("").route(
            self.path,
            web::get().to(move |req: HttpRequest, stream: web::Payload| {
                ws_service(protocols, Rc::clone(&modules), req, stream)
            }),
        ));
    }
}

async fn ws_service(
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

    let (protocol, _access_token) = match (protocol, access_token) {
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

    // TODO CHECK ACCESS TOKEN

    let mut response = ws::handshake(&req)?;

    let (adapter, actix_stream) = ActixTungsteniteAdapter::from_actix_payload(stream);
    let websocket = WebSocketStream::from_raw_socket(adapter, Role::Server, None).await;

    let mut builder = Modules::default();

    for module in modules.iter() {
        module.build(&mut builder, protocol).await;
    }

    task::spawn_local(Runner::new(builder, websocket).run());

    // TODO: maybe change the SEC_WEBSOCKET_PROTOCOL header to access_token=kdpaosd2eja9dj,k3k-signaling-json-v1 to avoid ordering
    response.insert_header((
        header::SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_str(protocol).unwrap(),
    ));

    Ok(response.streaming(actix_stream))
}
