use crate::api::v1::middleware::oidc_auth::check_access_token;
use crate::db::DbInterface;
use crate::modules::http::HttpModule;
use crate::oidc::OidcContext;
use actix_web::dev::Extensions;
use actix_web::http::{header, HeaderValue};
use actix_web::web::{Data, ServiceConfig};
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use adapter::ActixTungsteniteAdapter;
use async_tungstenite::tungstenite::protocol::Role;
use async_tungstenite::tungstenite::Message;
use async_tungstenite::WebSocketStream;
use futures::stream::SelectAll;
use modules::{any_stream, AnyStream, ModuleBuilder, ModuleBuilderImpl, Modules};
use openidconnect::AccessToken;
use runner::{Namespaced, Runner};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;
use tokio::task;
use tokio_stream::Stream;

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
pub struct WsCtx<'ctx, M>
where
    M: SignalingModule,
{
    ws_messages: &'ctx mut Vec<Message>,
    events: &'ctx mut SelectAll<AnyStream>,
    local_state: &'ctx mut Extensions,
    m: PhantomData<fn() -> M>,
}

impl<M> WsCtx<'_, M>
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

    /// Access to local state of the websocket
    pub fn local_state(&mut self) -> &mut Extensions {
        self.local_state
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
    type Params: Send + Sync;

    /// The websocket incoming message type
    type Incoming: for<'de> Deserialize<'de>;

    /// The websocket outgoing message type
    type Outgoing: Serialize;

    /// Optional event type, yielded by `ExtEventStream`
    ///
    /// If the module does not register external events it should be set to `()`.
    type ExtEvent;

    /// Constructor of the module.
    ///
    /// Provided with the websocket context the modules params and the negotiated protocol.
    ///
    /// Calls to [`WsCtx::ws_send`] are discarded
    async fn init(ctx: WsCtx<'_, Self>, params: &Self::Params, protocol: &'static str) -> Self;

    /// Events related to this module will be passed into this function together with [EventCtx]
    /// which gives access to the websocket and other related information.
    async fn on_event(&mut self, ctx: WsCtx<'_, Self>, event: Event<Self>);

    /// Before dropping the module this function will be called
    async fn on_destroy(self);
}

/// Signaling endpoint which can be registered to a `ApplicationBuilder`.
///
/// Logic is implemented via modules.
/// See [`SignalingModule`] and [`SignalingHttpModule::add_module`]
pub struct SignalingHttpModule {
    protocols: &'static [&'static str],
    modules: Vec<Box<dyn ModuleBuilder>>,
}

impl SignalingHttpModule {
    pub fn new() -> Self {
        Self {
            protocols: &["k3k-signaling-json-v1"],
            modules: Default::default(),
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
            params: Arc::new(params),
        }));
    }
}

impl HttpModule for SignalingHttpModule {
    fn register(&self, app: &mut ServiceConfig) {
        let protocols = self.protocols;
        let modules = Rc::from(self.modules.clone().into_boxed_slice());

        app.service(web::scope("").route(
            "/signaling",
            web::get().to(move |db_ctx, oidc_ctx, req, stream| {
                ws_service(
                    db_ctx,
                    oidc_ctx,
                    protocols,
                    Rc::clone(&modules),
                    req,
                    stream,
                )
            }),
        ));
    }
}

async fn ws_service(
    db_ctx: Data<DbInterface>,
    oidc_ctx: Data<OidcContext>,
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

    let mut builder = Modules::default();

    builder.local_state().insert(user);

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
