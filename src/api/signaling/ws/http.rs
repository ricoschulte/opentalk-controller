use super::adapter::ActixTungsteniteAdapter;
use super::modules::{ModuleBuilder, ModuleBuilderImpl};
use super::runner::Runner;
use super::ParticipantId;
use super::SignalingModule;
use crate::api::v1::rooms::{Ticket, TicketData};
use crate::api::v1::{ApiError, DefaultApiError};
use crate::db::rooms::Room;
use crate::db::users::User;
use crate::db::DbInterface;
use crate::modules::http::HttpModule;
use actix_web::http::{header, HeaderValue};
use actix_web::web::{Data, ServiceConfig};
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use async_tungstenite::tungstenite::protocol::Role;
use async_tungstenite::WebSocketStream;
use redis::aio::ConnectionManager;
use std::marker::PhantomData;
use std::rc::Rc;
use tokio::sync::broadcast;
use tokio::task;
use validator::Validate;

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
            web::get().to(move |shutdown, db_ctx, req, stream| {
                ws_service(
                    shutdown,
                    db_ctx,
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
    mut redis_conn: ConnectionManager,
    rabbit_mq_channel: lapin::Channel,
    protocols: &'static [&'static str],
    modules: Rc<[Box<dyn ModuleBuilder>]>,
    request: HttpRequest,
    stream: web::Payload,
) -> actix_web::Result<HttpResponse> {
    let (ticket, protocol) = read_request_header(&request, protocols)?;

    let ticket_data = get_ticket_data_from_redis(&mut redis_conn, &ticket).await?;

    let (user, room) = get_user_and_room_from_ticket(db_ctx.clone(), ticket_data).await?;

    let mut response = ws::handshake(&request)?;

    let (adapter, actix_stream) = ActixTungsteniteAdapter::from_actix_payload(stream);
    let websocket = WebSocketStream::from_raw_socket(adapter, Role::Server, None).await;

    let id = ParticipantId::new();

    let mut builder = Runner::builder(
        id,
        room,
        user,
        protocol,
        db_ctx.into_inner(),
        redis_conn,
        rabbit_mq_channel,
    );

    // add all modules
    for module in modules.iter() {
        if let Err(e) = module.build(&mut builder).await {
            log::error!("Failed to initialize module, {:?}", e);
            builder.abort().await;
            return Ok(HttpResponse::InternalServerError().finish());
        }
    }

    // Build and initialize the runner
    let runner = match builder.build(websocket, shutdown.subscribe()).await {
        Ok(runner) => runner,
        Err(e) => {
            log::error!("Failed to initialize runner, {}", e);
            return Ok(HttpResponse::InternalServerError().finish());
        }
    };

    // Spawn the runner task
    task::spawn_local(runner.run());

    response.insert_header((
        header::SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_str(protocol).unwrap(),
    ));

    Ok(response.streaming(actix_stream))
}

fn read_request_header(
    request: &HttpRequest,
    allowed_protocols: &'static [&'static str],
) -> Result<(Ticket, &'static str), ApiError> {
    let mut protocol = None;
    let mut ticket = None;

    // read the SEC_WEBSOCKET_PROTOCOL header
    if let Some(value) = request
        .headers()
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|v| v.to_str().ok())
    {
        let values = value.split(',').map(str::trim);

        for value in values {
            if value.starts_with("ticket#") {
                let (_, tmp_ticket) = value.split_once("#").unwrap();
                ticket = Some(Ticket {
                    ticket: tmp_ticket.into(),
                });
                continue;
            } else if protocol.is_some() {
                continue;
            } else if let Some(value) = allowed_protocols.iter().find(|&&v| v == value) {
                protocol = Some(value);
            }
        }
    }

    // look if valid protocol exists
    let protocol = match protocol {
        Some(protocol) => protocol,
        None => {
            log::debug!("Rejecting websocket request, missing valid protocol");
            return Err(DefaultApiError::BadRequest("Missing protocol".into()));
        }
    };

    // look if ticket exists
    let ticket = match ticket {
        Some(ticket) => match ticket.validate() {
            Ok(_) => ticket,
            Err(e) => {
                log::warn!("Ticket validation failed for {:?}, {}", ticket, e);

                return Err(DefaultApiError::auth_bearer_invalid_token(
                    "ticket invalid",
                    "Please request a new ticket from /v1/rooms/<room_uuid>/start".into(),
                ));
            }
        },
        None => {
            log::debug!("Rejecting websocket request, missing ticket");
            return Err(DefaultApiError::auth_bearer_invalid_request(
                "missing ticket",
                "Please request a ticket from /v1/rooms/<room_uuid>/start".into(),
            ));
        }
    };

    Ok((ticket, protocol))
}

async fn get_ticket_data_from_redis(
    redis_conn: &mut ConnectionManager,
    ticket: &Ticket,
) -> Result<TicketData, ApiError> {
    // GETDEL available since redis 6.2.0, missing direct support by redis crate
    let ticket_data: Option<TicketData> = redis::cmd("GETDEL")
        .arg(ticket)
        .query_async(redis_conn)
        .await
        .map_err(|e| {
            log::warn!("Unable to get ticket data in redis: {}", e);
            DefaultApiError::Internal
        })?;

    let ticket_data = ticket_data.ok_or_else(|| {
        DefaultApiError::auth_bearer_invalid_token(
            "ticket invalid or expired",
            "Please request a new ticket from /v1/rooms/<room_uuid>/start".into(),
        )
    })?;

    Ok(ticket_data)
}

async fn get_user_and_room_from_ticket(
    db_ctx: Data<DbInterface>,
    ticket_data: TicketData,
) -> Result<(User, Room), ApiError> {
    web::block(move || -> Result<(User, Room), DefaultApiError> {
        let user = db_ctx
            .get_user_by_uuid(&ticket_data.user)
            .map_err(DefaultApiError::from)?;

        let user = user.ok_or(DefaultApiError::Internal)?;

        let room = db_ctx
            .get_room_by_uuid(&ticket_data.room)
            .map_err(DefaultApiError::from)?;

        let room = room.ok_or(DefaultApiError::NotFound)?;

        Ok((user, room))
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on ws_service - {}", e);
        DefaultApiError::Internal
    })?
}
