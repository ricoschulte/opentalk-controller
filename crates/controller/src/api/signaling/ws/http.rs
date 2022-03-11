use super::adapter::ActixTungsteniteAdapter;
use super::modules::{ModuleBuilder, ModuleBuilderImpl};
use super::runner::Runner;
use super::SignalingModule;
use crate::api::signaling::resumption::{ResumptionData, ResumptionTokenKeepAlive};
use crate::api::signaling::ticket::{TicketData, TicketRedisKey};
use crate::api::v1::{ApiError, DefaultApiError};
use crate::api::Participant;
use actix_web::http::header;
use actix_web::web::Data;
use actix_web::{get, HttpMessage};
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use async_tungstenite::tungstenite::protocol::Role;
use async_tungstenite::WebSocketStream;
use database::Db;
use db_storage::rooms::DbRoomsEx;
use db_storage::rooms::Room;
use db_storage::users::User;
use db_storage::DbUsersEx;
use kustos::Authz;
use redis::aio::ConnectionManager;
use std::marker::PhantomData;
use tokio::sync::broadcast;
use tokio::task;
use tracing_actix_web::RequestId;

#[derive(Default)]
pub struct SignalingModules(Vec<Box<dyn ModuleBuilder>>);

impl SignalingModules {
    pub fn add_module<M>(&mut self, params: M::Params)
    where
        M: SignalingModule + 'static,
    {
        self.0.push(Box::new(ModuleBuilderImpl {
            m: PhantomData::<fn() -> M>,
            params,
        }));
    }
}

pub struct SignalingProtocols(&'static [&'static str]);

impl SignalingProtocols {
    pub fn data() -> Data<Self> {
        Data::new(Self(&["k3k-signaling-json-v1.0"]))
    }
}

#[allow(clippy::too_many_arguments)]
#[get("/signaling")]
pub(crate) async fn ws_service(
    shutdown: Data<broadcast::Sender<()>>,
    db: Data<Db>,
    authz: Data<Authz>,
    redis_conn: Data<ConnectionManager>,
    rabbit_mq_channel: Data<lapin::Channel>,
    protocols: Data<SignalingProtocols>,
    modules: Data<SignalingModules>,
    request: HttpRequest,
    stream: web::Payload,
) -> actix_web::Result<HttpResponse> {
    let mut redis_conn = (**redis_conn).clone();

    let request_id = if let Some(request_id) = request.extensions().get::<RequestId>() {
        **request_id
    } else {
        log::error!("missing request id in signaling request");
        return Ok(HttpResponse::InternalServerError().finish());
    };

    // Read ticket and protocol from protocol header
    let (ticket, protocol) = read_request_header(&request, protocols.0)?;

    // Read ticket data from redis
    let ticket_data = get_ticket_data_from_redis(&mut redis_conn, ticket).await?;

    // Get user & room from database using the ticket data
    let (participant, room) = get_user_and_room_from_ticket_data(db.clone(), &ticket_data).await?;

    // Create resumption data to be refreshed by the runner in redis
    let resumption_data = ResumptionData {
        participant_id: ticket_data.participant_id,
        participant: match &participant {
            Participant::User(user) => Participant::User(user.id),
            Participant::Guest => Participant::Guest,
            Participant::Sip => Participant::Sip,
        },
        room: ticket_data.room,
        breakout_room: ticket_data.breakout_room,
    };

    // Create keep-alive util for resumption data
    let resumption_keep_alive =
        ResumptionTokenKeepAlive::new(ticket_data.resumption, resumption_data);

    // Finish websocket handshake
    let mut response = ws::handshake(&request)?;

    let (adapter, actix_stream) = ActixTungsteniteAdapter::from_actix_payload(stream);
    let websocket = WebSocketStream::from_raw_socket(adapter, Role::Server, None).await;

    let mut builder = Runner::builder(
        request_id,
        ticket_data.participant_id,
        room,
        ticket_data.breakout_room,
        participant,
        protocol,
        db.into_inner(),
        authz.into_inner(),
        redis_conn,
        (**rabbit_mq_channel).clone(),
        resumption_keep_alive,
    );

    // add all modules
    for module in modules.0.iter() {
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

    response.insert_header((header::SEC_WEBSOCKET_PROTOCOL, protocol));

    Ok(response.streaming(actix_stream))
}

fn read_request_header<'t>(
    request: &'t HttpRequest,
    allowed_protocols: &'static [&'static str],
) -> Result<(TicketRedisKey<'t>, &'static str), ApiError> {
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
                let (_, tmp_ticket) = value.split_once('#').unwrap();
                ticket = Some(tmp_ticket);
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

    // verify ticket existence and validity
    let ticket = match ticket {
        Some(ticket) if ticket.len() == 64 => ticket,
        Some(ticket) => {
            log::warn!(
                "got ticket with invalid ticket length expected 64, got {} ",
                ticket.len()
            );

            return Err(DefaultApiError::auth_bearer_invalid_token(
                "ticket invalid",
                "Please request a new ticket from /v1/rooms/<room_id>/start".into(),
            ));
        }
        None => {
            log::debug!("Rejecting websocket request, missing ticket");
            return Err(DefaultApiError::auth_bearer_invalid_request(
                "missing ticket",
                "Please request a ticket from /v1/rooms/<room_id>/start".into(),
            ));
        }
    };

    Ok((TicketRedisKey { ticket }, protocol))
}

async fn get_ticket_data_from_redis(
    redis_conn: &mut ConnectionManager,
    ticket: TicketRedisKey<'_>,
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
            "Please request a new ticket from /v1/rooms/<room_id>/start".into(),
        )
    })?;

    Ok(ticket_data)
}

async fn get_user_and_room_from_ticket_data(
    db: Data<Db>,
    ticket_data: &TicketData,
) -> Result<(Participant<User>, Room), ApiError> {
    let participant = ticket_data.participant;
    let room_id = ticket_data.room;

    crate::block(
        move || -> Result<(Participant<User>, Room), DefaultApiError> {
            let participant = match participant {
                Participant::User(user_id) => {
                    let user = db
                        .get_opt_user_by_id(user_id)?
                        .ok_or(DefaultApiError::Internal)?;

                    Participant::User(user)
                }
                Participant::Guest => Participant::Guest,
                Participant::Sip => Participant::Sip,
            };

            let room = db.get_room(room_id).map_err(DefaultApiError::from)?;

            let room = room.ok_or(DefaultApiError::NotFound)?;

            Ok((participant, room))
        },
    )
    .await
    .map_err(|e| {
        log::error!("BlockingError on ws_service - {}", e);
        DefaultApiError::Internal
    })?
}
