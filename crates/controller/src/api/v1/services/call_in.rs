use crate::api::signaling::resumption::ResumptionToken;
use crate::api::signaling::ticket::{start_or_continue_signaling_session, TicketToken};
use crate::api::v1::response::ApiError;
use crate::api::Participant;
use crate::redis_wrapper::RedisConnection;
use actix_web::dev::HttpServiceFactory;
use actix_web::error::Result;
use actix_web::post;
use actix_web::web::{Data, Json};
use database::Db;
use db_storage::rooms::RoomId;
use db_storage::sip_configs::{SipConfig, SipId, SipPassword};
use serde::{Deserialize, Serialize};
use validator::Validate;

pub const REQUIRED_CALL_IN_ROLE: &str = "opentalk-call-in";

#[derive(Deserialize)]
pub struct CallInStartRequestBody {
    id: SipId,
    pin: SipPassword,
}

#[derive(Serialize)]
pub struct CallInStartResponse {
    ticket: TicketToken,
    resumption: ResumptionToken,
}

/// API Endpoint *POST services/call_in/start* for the call-in service
#[post("/start")]
pub async fn start(
    db: Data<Db>,
    redis_ctx: Data<RedisConnection>,
    request: Json<CallInStartRequestBody>,
) -> Result<Json<CallInStartResponse>, ApiError> {
    let mut redis_conn = (**redis_ctx).clone();
    let request = request.into_inner();

    request.id.validate()?;
    request.pin.validate()?;

    let room_id = crate::block(move || -> Result<RoomId, ApiError> {
        let mut conn = db.get_conn()?;

        if let Some(sip_config) = SipConfig::get(&mut conn, request.id)? {
            if sip_config.password == request.pin {
                return Ok(sip_config.room);
            }
        }

        Err(ApiError::bad_request()
            .with_code("invalid_credentials")
            .with_message("given call-in id & pin combination is not valid"))
    })
    .await??;

    let (ticket, resumption) =
        start_or_continue_signaling_session(&mut redis_conn, Participant::Sip, room_id, None, None)
            .await?;

    Ok(Json(CallInStartResponse { ticket, resumption }))
}

pub fn services() -> impl HttpServiceFactory {
    actix_web::web::scope("/call_in")
        .wrap(super::RequiredRealmRole::new(REQUIRED_CALL_IN_ROLE))
        .service(start)
}
