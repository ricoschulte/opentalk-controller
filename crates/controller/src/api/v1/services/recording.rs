// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::api::signaling::resumption::ResumptionToken;
use crate::api::signaling::ticket::start_or_continue_signaling_session;
use crate::api::signaling::ticket::TicketToken;
use crate::api::v1::response::ApiError;
use crate::api::v1::response::NoContent;
use crate::api::Participant;
use crate::redis_wrapper::RedisConnection;
use crate::settings::SharedSettingsActix;
use crate::storage::assets::save_asset;
use crate::storage::ObjectStorage;
use actix_web::dev::HttpServiceFactory;
use actix_web::post;
use actix_web::web::Payload;
use actix_web::web::Query;
use actix_web::web::{Data, Json};
use database::Db;
use db_storage::rooms::Room;
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use types::core::RoomId;

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

#[derive(Deserialize)]
pub struct UploadRenderQuery {
    room_id: RoomId,
    filename: String,
}

#[post("/upload_render")]
pub async fn upload_render(
    storage: Data<ObjectStorage>,
    db: Data<Db>,
    query: Query<UploadRenderQuery>,
    data: Payload,
) -> Result<NoContent, ApiError> {
    // Assert that the room exists
    crate::block({
        let db = db.clone();
        let room_id = query.room_id;
        move || {
            let mut conn = db.get_conn()?;

            Room::get(&mut conn, room_id)
        }
    })
    .await??;

    save_asset(
        &storage,
        db.into_inner(),
        query.room_id,
        Some("recording"),
        &query.filename,
        "recording-render",
        data.into_stream().map_err(anyhow::Error::from),
    )
    .await?;

    Ok(NoContent)
}

pub fn services() -> impl HttpServiceFactory {
    actix_web::web::scope("/recording")
        .wrap(super::RequiredRealmRole::new(REQUIRED_RECORDING_ROLE))
        .service(start)
        .service(upload_render)
}
