use crate::api::signaling::resumption::ResumptionToken;
use crate::api::signaling::ticket::start_or_continue_signaling_session;
use crate::api::signaling::ticket::TicketToken;
use crate::api::v1::response::ApiError;
use crate::api::Participant;
use crate::redis_wrapper::RedisConnection;
use crate::settings::SharedSettingsActix;
use actix_web::dev::HttpServiceFactory;
use actix_web::post;
use actix_web::web::{Data, Json};
use database::Db;
use db_storage::rooms::{Room, RoomId};
use serde::{Deserialize, Serialize};

const REQUIRED_RECORDING_ROLE: &str = "opentalk-recorder";

#[derive(Debug, Deserialize)]
pub struct RecorderStartBody {
    room_id: RoomId,
}

#[derive(Serialize)]
pub struct RecordingStartResponse {
    ticket: TicketToken,
    resumption: ResumptionToken,
}

#[post("/start")]
pub async fn start(
    settings: SharedSettingsActix,
    db: Data<Db>,
    redis_ctx: Data<RedisConnection>,
    body: Json<RecorderStartBody>,
) -> Result<Json<RecordingStartResponse>, ApiError> {
    let settings = settings.load_full();
    if settings.rabbit_mq.recording_task_queue.is_none() {
        return Err(ApiError::not_found());
    }

    let mut redis_conn = (**redis_ctx).clone();
    let body = body.into_inner();

    let room = crate::block(move || -> database::Result<_> {
        let mut conn = db.get_conn()?;

        Room::get(&mut conn, body.room_id)
    })
    .await??;

    let (ticket, resumption) = start_or_continue_signaling_session(
        &mut redis_conn,
        Participant::Recorder,
        room.id,
        None,
        None,
    )
    .await?;

    Ok(Json(RecordingStartResponse { ticket, resumption }))
}

pub fn services() -> impl HttpServiceFactory {
    actix_web::web::scope("/recording")
        .wrap(super::RequiredRealmRole::new(REQUIRED_RECORDING_ROLE))
        .service(start)
}
