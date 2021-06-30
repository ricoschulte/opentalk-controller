use super::adapter::ActixTungsteniteAdapter;
use super::modules::{ModuleBuilder, ModuleBuilderImpl};
use super::runner::Runner;
use super::ParticipantId;
use super::SignalingModule;
use crate::api::v1::middleware::oidc_auth::check_access_token;
use crate::db::DbInterface;
use crate::modules::http::HttpModule;
use crate::oidc::OidcContext;
use actix_web::http::{header, HeaderValue};
use actix_web::web::{Data, ServiceConfig};
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use async_tungstenite::tungstenite::protocol::Role;
use async_tungstenite::WebSocketStream;
use openidconnect::AccessToken;
use redis::aio::ConnectionManager;
use std::marker::PhantomData;
use std::rc::Rc;
use tokio::sync::broadcast;
use tokio::task;
use uuid::Uuid;

/// Signaling endpoint which can be registered to a `ApplicationBuilder`.
///
/// Logic is implemented via modules.
/// See [`SignalingModule`] and [`SignalingHttpModule::add_module`]
pub struct SignalingHttpModule {
    protocols: &'static [&'static str],
    modules: Vec<Box<dyn ModuleBuilder>>,

    redis_conn: ConnectionManager,
    rabbit_mq_channel: lapin::Channel,
}

impl SignalingHttpModule {
    pub fn new(redis_conn: ConnectionManager, rabbit_mq_channel: lapin::Channel) -> Self {
        Self {
            protocols: &["k3k-signaling-json-v1.0"],
            modules: Default::default(),
            redis_conn,
            rabbit_mq_channel,
        }
    }

    /// Add a implementation of [`SignalingModule`] together with some options used to initialize
    /// the module.
    ///
    /// The module itself will be created on successful websocket connection.
    pub fn with_module<M>(mut self, params: M::Params) -> Self
    where
        M: SignalingModule + 'static,
    {
        self.modules.push(Box::new(ModuleBuilderImpl {
            m: PhantomData::<fn() -> M>,
            params,
        }));

        self
    }
}

impl HttpModule for SignalingHttpModule {
    fn register(&self, app: &mut ServiceConfig) {
        // The service needs some data from this module.
        // Since a service must be Fn + 'static the data must be cloned
        // into the closure, where for every call we clone it again
        // into the ws_service function

        let protocols = self.protocols;
        let modules = Rc::from(self.modules.clone().into_boxed_slice());
        let redis_conn = self.redis_conn.clone();
        let rabbit_mq_channel = self.rabbit_mq_channel.clone();

        app.service(web::scope("").route(
            "/signaling",
            web::get().to(move |shutdown, db_ctx, oidc_ctx, req, stream| {
                ws_service(
                    shutdown,
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
    shutdown: Data<broadcast::Sender<()>>,
    db_ctx: Data<DbInterface>,
    oidc_ctx: Data<OidcContext>,
    redis_conn: ConnectionManager,
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
        _ => {
            log::debug!("Rejecting request, missing access_token or invalid protocol");

            return Ok(HttpResponse::Forbidden().finish());
        }
    };

    let user = check_access_token(db_ctx, oidc_ctx, AccessToken::new(access_token.into())).await?;

    let mut response = ws::handshake(&req)?;

    let (adapter, actix_stream) = ActixTungsteniteAdapter::from_actix_payload(stream);
    let websocket = WebSocketStream::from_raw_socket(adapter, Role::Server, None).await;

    // TODO: Get these information from somewhere reliable (ticket-system + redis)?
    let id = ParticipantId::new();
    let room = Uuid::nil();

    let mut builder = Runner::builder(id, room, user, protocol, redis_conn, rabbit_mq_channel);

    // add all modules
    for module in modules.iter() {
        if let Err(e) = module.build(&mut builder).await {
            log::error!("Failed to initialize module, {:?}", e);
            builder.abort().await;
            return HttpResponse::InternalServerError().await;
        }
    }

    // Build and initialize the runner
    let runner = match builder.build(websocket, shutdown.subscribe()).await {
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
