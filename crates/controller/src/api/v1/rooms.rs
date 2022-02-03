//! Room related API structs and Endpoints
//!
//! The defined structs are exposed to the REST API and will be serialized/deserialized. Similar
//! structs are defined in the Database module [`crate::db`] for database operations.

use super::response::NoContent;
use crate::api::signaling::prelude::*;
use crate::api::signaling::resumption::{ResumptionData, ResumptionToken};
use crate::api::signaling::ticket::{TicketData, TicketToken};
use crate::api::v1::{ApiError, ApiResponse, DefaultApiError, PagePaginationQuery};
use crate::api::Participant;
use actix_web::web::{self, Data, Json, Path, ReqData};
use actix_web::{delete, get, post, put};
use anyhow::Context;
use controller_shared::ParticipantId;
use database::Db;
use db_storage::invites::DbInvitesEx;
use db_storage::invites::InviteCodeId;
use db_storage::rooms::DbRoomsEx;
use db_storage::rooms::{self as db_rooms, RoomId};
use db_storage::sip_configs::DbSipConfigsEx;
use db_storage::sip_configs::{SipConfigParams, SipId, SipPassword};
use db_storage::users::{User, UserId};
use kustos::prelude::*;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use validator::{Validate, ValidationError};

/// A Room
///
/// Contains all room information. Is only be accessible to the owner and users with
/// appropriate permissions.
#[derive(Debug, Serialize)]
pub struct Room {
    pub uuid: RoomId,
    pub owner: UserId,
    pub password: String,
    pub wait_for_moderator: bool,
    pub listen_only: bool,
}

/// Public room details
///
/// Contains general public information about a room.
#[derive(Debug, Serialize)]
pub struct RoomDetails {
    pub uuid: RoomId,
    pub owner: UserId,
    pub wait_for_moderator: bool,
    pub listen_only: bool,
}

/// API request parameters to create a new room
#[derive(Debug, Validate, Deserialize)]
pub struct NewRoom {
    #[validate(length(max = 255))]
    pub password: String,
    pub wait_for_moderator: bool,
    pub listen_only: bool,
    /// Enable/Disable sip for this room; defaults to false when not set
    #[serde(default)]
    pub enable_sip: bool,
}

/// API request parameters to modify a room.
#[derive(Debug, Validate, Deserialize)]
#[validate(schema(function = "disallow_empty"))]
pub struct ModifyRoom {
    #[validate(length(max = 255))]
    pub password: Option<String>,
    pub wait_for_moderator: Option<bool>,
    pub listen_only: Option<bool>,
}

fn disallow_empty(modify_room: &ModifyRoom) -> Result<(), ValidationError> {
    let ModifyRoom {
        password,
        wait_for_moderator,
        listen_only,
    } = modify_room;

    if password.is_none() && wait_for_moderator.is_none() && listen_only.is_none() {
        Err(ValidationError::new("ModifyRoom has no set fields"))
    } else {
        Ok(())
    }
}

/// API Endpoint *GET /rooms*
///
/// Returns a JSON array of all owned rooms as [`Room`]
#[get("/rooms")]
pub async fn owned(
    db: Data<Db>,
    current_user: ReqData<User>,
    pagination: web::Query<PagePaginationQuery>,
    authz: Data<Authz>,
) -> Result<ApiResponse<Vec<Room>>, DefaultApiError> {
    let current_user = current_user.into_inner();
    let PagePaginationQuery { per_page, page } = pagination.into_inner();

    let accessible_rooms: kustos::AccessibleResources<RoomId> = authz
        .get_accessible_resources_for_user(current_user.id, AccessMethod::Get)
        .await
        .map_err(|_| DefaultApiError::Internal)?;

    let (rooms, room_count) = crate::block(
        move || -> Result<(Vec<db_rooms::Room>, i64), DefaultApiError> {
            match accessible_rooms {
                kustos::AccessibleResources::All => Ok(db.get_rooms_paginated(per_page, page)?),
                kustos::AccessibleResources::List(list) => {
                    Ok(db.get_rooms_by_ids_paginated(&list, per_page as i64, page as i64)?)
                }
            }
        },
    )
    .await??;

    let rooms = rooms
        .into_iter()
        .map(|db_room| Room {
            uuid: db_room.id,
            owner: db_room.created_by,
            password: db_room.password,
            wait_for_moderator: db_room.wait_for_moderator,
            listen_only: db_room.listen_only,
        })
        .collect::<Vec<Room>>();

    Ok(ApiResponse::new(rooms).with_page_pagination(per_page, page, room_count))
}

/// API Endpoint *POST /rooms*
///
/// Uses the provided [`NewRoom`] to create a new room.
/// Returns the created [`Room`].
#[post("/rooms")]
pub async fn new(
    db: Data<Db>,
    current_user: ReqData<User>,
    room_parameters: Json<NewRoom>,
    authz: Data<Authz>,
) -> Result<Json<Room>, DefaultApiError> {
    let room_parameters = room_parameters.into_inner();

    if let Err(e) = room_parameters.validate() {
        log::warn!("API new room validation error {}", e);
        return Err(DefaultApiError::ValidationFailed);
    }

    let current_user_id = current_user.id;
    let db_room = crate::block(move || -> Result<db_rooms::Room, DefaultApiError> {
        let new_room = db_rooms::NewRoom {
            created_by: current_user_id,
            password: room_parameters.password,
            wait_for_moderator: room_parameters.wait_for_moderator,
            listen_only: room_parameters.listen_only,
        };

        let room = db.new_room(new_room)?;

        if room_parameters.enable_sip {
            let sip_params = SipConfigParams::generate_new(room.id);

            db.new_sip_config(sip_params)?;
        }

        Ok(room)
    })
    .await??;

    let room = Room {
        uuid: db_room.id,
        owner: db_room.created_by,
        password: db_room.password,
        wait_for_moderator: db_room.wait_for_moderator,
        listen_only: db_room.listen_only,
    };

    if let Err(e) = authz
        .grant_user_access(
            current_user.id,
            &[
                (
                    &db_room.id.resource_id(),
                    &[AccessMethod::Get, AccessMethod::Put, AccessMethod::Delete],
                ),
                (
                    &db_room.id.resource_id().with_suffix("/invites"),
                    &[AccessMethod::Post, AccessMethod::Get],
                ),
                (
                    &db_room.id.resource_id().with_suffix("/start"),
                    &[AccessMethod::Post],
                ),
            ],
        )
        .await
    {
        log::error!("Failed to add RBAC policy: {}", e);
        return Err(DefaultApiError::Internal);
    }

    Ok(Json(room))
}

/// API Endpoint *PUT /rooms/{room_uuid}*
///
/// Uses the provided [`ModifyRoom`] to modify a specified room.
/// Returns the modified [`Room`]
#[put("/rooms/{room_uuid}")]
pub async fn modify(
    db: Data<Db>,
    current_user: ReqData<User>,
    room_id: Path<RoomId>,
    modify_room: Json<ModifyRoom>,
) -> Result<Json<Room>, DefaultApiError> {
    let room_id = room_id.into_inner();
    let modify_room = modify_room.into_inner();

    if let Err(e) = modify_room.validate() {
        log::warn!("API modify room validation error {}", e);
        return Err(DefaultApiError::ValidationFailed);
    }

    let db_room = crate::block(move || {
        let room = db.get_room(room_id)?;

        match room {
            None => Err(DefaultApiError::NotFound),
            Some(room) => {
                // TODO: check user permissions when implemented
                if room.created_by != current_user.id {
                    return Err(DefaultApiError::InsufficientPermission);
                }

                let change_room = db_rooms::UpdateRoom {
                    password: modify_room.password,
                    wait_for_moderator: modify_room.wait_for_moderator,
                    listen_only: modify_room.listen_only,
                };

                Ok(db.modify_room(room_id, change_room)?)
            }
        }
    })
    .await??;

    let room = Room {
        uuid: db_room.id,
        owner: db_room.created_by,
        password: db_room.password,
        wait_for_moderator: db_room.wait_for_moderator,
        listen_only: db_room.listen_only,
    };

    Ok(Json(room))
}

/// API Endpoint *DELETE /rooms/{room_uuid}*
///
/// Deletes the room and owned resources.
#[delete("/rooms/{room_uuid}")]
pub async fn delete(
    db: Data<Db>,
    room_id: Path<RoomId>,
    authz: Data<Authz>,
) -> Result<NoContent, DefaultApiError> {
    let room_id = room_id.into_inner();
    let room_path = format!("/rooms/{}", room_id);

    crate::block(move || db.delete_room(room_id)).await??;

    if !authz
        .remove_explicit_resource_permissions(room_path.clone())
        .await
        .map_err(|_| DefaultApiError::Internal)?
    {
        log::error!("Failed to delete permissions for {}", room_path);
    }

    Ok(NoContent {})
}

/// API Endpoint *GET /rooms/{room_uuid}*
///
/// Returns the specified Room as [`RoomDetails`].
#[get("/rooms/{room_uuid}")]
pub async fn get(
    db: Data<Db>,
    room_id: Path<RoomId>,
) -> Result<Json<RoomDetails>, DefaultApiError> {
    let room_id = room_id.into_inner();

    let db_room = crate::block(move || {
        let room = db.get_room(room_id)?;

        match room {
            None => Err(DefaultApiError::NotFound),
            Some(room) => Ok(room),
        }
    })
    .await??;

    let room_details = RoomDetails {
        uuid: db_room.id,
        owner: db_room.created_by,
        wait_for_moderator: db_room.wait_for_moderator,
        listen_only: db_room.listen_only,
    };

    Ok(Json(room_details))
}

/// The JSON body expected when making a *POST /rooms/{room_uuid}/start*
#[derive(Debug, Deserialize)]
pub struct StartRequest {
    password: Option<String>,
    breakout_room: Option<BreakoutRoomId>,
    resumption: Option<ResumptionToken>,
}

/// The JSON body returned from the start endpoints supporting session resumption
#[derive(Debug, Serialize)]
pub struct StartResponse {
    ticket: TicketToken,
    resumption: ResumptionToken,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StartRoomError {
    InvalidInvite,
    WrongRoomPassword,
    InvalidCredentials,
    NoBreakoutRooms,
    InvalidBreakoutRoomId,
}

type StartError = ApiError<StartRoomError>;

/// API Endpoint *POST /rooms/{room_id}/start*
///
/// This endpoint has to be called in order to get a [`Ticket`]. When joining a room, the ticket
/// must be provided as a `Sec-WebSocket-Protocol` header field when starting the WebSocket
/// connection.
///
/// When the requested room has a password set, the requester has to provide the correct password
/// through the [`StartRequest`] JSON in the requests body. When the room has no password set,
/// the provided password will be ignored.
///
/// Returns a [`Ticket`] for the specified room.
///
/// # Errors
///
/// Returns [`StartError::NotFound`](ApiError::NotFound) when the requested room could not be found.
/// Returns [`StartRoomError::WrongRoomPassword`] when the provided password is wrong.
#[post("/rooms/{room_id}/start")]
pub async fn start(
    db: Data<Db>,
    redis_ctx: Data<ConnectionManager>,
    current_user: ReqData<User>,
    room_id: Path<RoomId>,
    request: Json<StartRequest>,
) -> Result<Json<StartResponse>, StartError> {
    let request = request.into_inner();
    let room_id = room_id.into_inner();

    let room = crate::block(move || -> Result<db_rooms::Room, StartError> {
        let room = db.get_room(room_id)?.ok_or(StartError::NotFound)?;

        Ok(room)
    })
    .await??;

    if !room.password.is_empty() {
        if let Some(pw) = &request.password {
            if pw != &room.password {
                return Err(StartError::AuthJson(
                    StartRoomError::WrongRoomPassword.into(),
                ));
            }
        } else {
            return Err(StartError::AuthJson(
                StartRoomError::WrongRoomPassword.into(),
            ));
        }
    }

    let mut redis_conn = (**redis_ctx).clone();

    if let Some(breakout_room) = request.breakout_room {
        let config = breakout::storage::get_config(&mut redis_conn, room.id).await?;

        if let Some(config) = config {
            if !config.is_valid_id(breakout_room) {
                return Err(StartError::AuthJson(
                    StartRoomError::InvalidBreakoutRoomId.into(),
                ));
            }
        } else {
            return Err(StartError::AuthJson(StartRoomError::NoBreakoutRooms.into()));
        }
    }

    let response = generate_response(
        &mut redis_conn,
        current_user.id.into(),
        room_id,
        request.breakout_room,
        request.resumption,
    )
    .await?;

    Ok(Json(response))
}

/// The JSON body expected when making a *POST /rooms/{room_uuid}/start_invited*
#[derive(Debug, Deserialize)]
pub struct InvitedStartRequest {
    password: Option<String>,
    invite_code: String,
    breakout_room: Option<BreakoutRoomId>,
    resumption: Option<ResumptionToken>,
}

/// API Endpoint *POST /rooms/{room_id}/start_invited*
///
/// See [`start`]
#[post("/rooms/{room_id}/start_invited")]
pub async fn start_invited(
    db: Data<Db>,
    redis_ctx: Data<ConnectionManager>,
    room_id: Path<RoomId>,
    request: Json<InvitedStartRequest>,
) -> Result<ApiResponse<StartResponse>, StartError> {
    let request = request.into_inner();
    let room_id = room_id.into_inner();

    let invite_code_as_uuid = uuid::Uuid::from_str(&request.invite_code)
        .map_err(|_| StartError::BadRequest("bad invite_code format".to_string()))?;

    let room = crate::block(move || -> Result<db_rooms::Room, StartError> {
        let invite = db.get_invite(InviteCodeId::from(invite_code_as_uuid))?;

        if !invite.active {
            return Err(StartError::AuthJson(StartRoomError::InvalidInvite.into()));
        }
        if invite.room != room_id {
            return Err(StartError::BadRequest("RoomId mismatch".to_string()));
        }
        let room = db.get_room(invite.room)?.ok_or(StartError::NotFound)?;

        Ok(room)
    })
    .await??;

    if !room.password.is_empty() {
        if let Some(pw) = &request.password {
            if pw != &room.password {
                return Err(StartError::AuthJson(
                    StartRoomError::WrongRoomPassword.into(),
                ));
            }
        } else {
            return Err(StartError::AuthJson(
                StartRoomError::WrongRoomPassword.into(),
            ));
        }
    }

    let mut redis_conn = (**redis_ctx).clone();

    if let Some(breakout_room) = request.breakout_room {
        let config = breakout::storage::get_config(&mut redis_conn, room.id).await?;

        if let Some(config) = config {
            if !config.is_valid_id(breakout_room) {
                return Err(StartError::AuthJson(
                    StartRoomError::InvalidBreakoutRoomId.into(),
                ));
            }
        } else {
            return Err(StartError::AuthJson(StartRoomError::NoBreakoutRooms.into()));
        }
    }

    let ticket = generate_response(
        &mut redis_conn,
        Participant::Guest,
        room_id,
        request.breakout_room,
        request.resumption,
    )
    .await?;

    Ok(ApiResponse::new(ticket))
}

#[derive(Debug, Deserialize)]
pub struct SipStartRequest {
    sip_id: SipId,
    password: SipPassword,
}

/// API Endpoint *POST /rooms/sip/start*
///
/// Get a [`Ticket`] for a new sip connection to a room. The requester has to provide
/// a valid [`SipId`] & [`SipPassword`] via the [`SipStartRequest`]
///
/// Returns [`StartError::NotFound`](ApiError::NotFound) when the requested room could not be found.
#[post("/rooms/sip/start")]
pub async fn sip_start(
    db: Data<Db>,
    redis_ctx: Data<ConnectionManager>,
    request: Json<SipStartRequest>,
) -> Result<ApiResponse<StartResponse>, StartError> {
    let mut redis_conn = (**redis_ctx).clone();
    let request = request.into_inner();

    request
        .sip_id
        .validate()
        .map_err(|_| StartError::BadRequest("bad sip_id format".to_string()))?;

    request
        .password
        .validate()
        .map_err(|_| StartError::BadRequest("bad sip_password format".to_string()))?;

    let room_id = crate::block(move || -> Result<RoomId, StartError> {
        if let Some(sip_config) = db.get_sip_config_by_sip_id(request.sip_id)? {
            if sip_config.password == request.password {
                let room = db.get_room(sip_config.room)?.ok_or(StartError::Internal)?;

                Ok(room.id)
            } else {
                Err(StartError::AuthJson(
                    StartRoomError::InvalidCredentials.into(),
                ))
            }
        } else {
            Err(StartError::AuthJson(
                StartRoomError::InvalidCredentials.into(),
            ))
        }
    })
    .await??;

    let response =
        generate_response(&mut redis_conn, Participant::Sip, room_id, None, None).await?;

    Ok(ApiResponse::new(response))
}

/// Generates a [`StartResponse`] from a given participant, room id, optional breakout room id and optional resumption token
///
/// Stores the generated ticket in redis together with its ticket data. The redis
/// key expires after 30 seconds.
///
/// If the given resumption token is correct, a exit-msg is sent via rabbitmq to the runner of the to-resume session.
async fn generate_response(
    redis_conn: &mut ConnectionManager,
    participant: Participant<UserId>,
    room: RoomId,
    breakout_room: Option<BreakoutRoomId>,
    resumption: Option<ResumptionToken>,
) -> Result<StartResponse, StartError> {
    // Get participant id, check resumption token if it exists, if not generate random one
    let participant_id = if let Some(resumption) = resumption {
        let resumption_redis_key = resumption.into_redis_key();

        // Check for resumption data behind resumption token
        let resumption_data: Option<ResumptionData> =
            redis_conn.get(&resumption_redis_key).await.map_err(|e| {
                log::error!("Failed to fetch resumption token from redis, {}", e);
                StartError::Internal
            })?;

        // If redis returned None generate random id, else check if request matches resumption data
        if let Some(data) = resumption_data {
            if data.room == room && data.participant == participant {
                let in_use =
                    control::storage::participant_id_in_use(redis_conn, data.participant_id)
                        .await?;

                if in_use {
                    return Err(StartError::BadRequest(
                        "the session of the given resumption token is still running".into(),
                    ));
                } else if redis_conn
                    .del(&resumption_redis_key)
                    .await
                    .context("failed to remove resumption token")?
                {
                    data.participant_id
                } else {
                    // edge case: we successfully GET the resumption token but failed to delete it from redis
                    // This can only be caused by the same endpoint being called at the same time (race condition)
                    // or the resumption data expiring at the same time as this endpoint was called
                    ParticipantId::new()
                }
            } else {
                log::debug!("given resumption was valid but was used in an invalid context (wrong user/room)");
                ParticipantId::new()
            }
        } else {
            log::debug!("given resumption was invalid, ignoring it");
            ParticipantId::new()
        }
    } else {
        // No resumption token given
        ParticipantId::new()
    };

    let ticket = TicketToken::generate();
    let resumption = ResumptionToken::generate();

    let ticket_data = TicketData {
        participant_id,
        participant,
        room,
        breakout_room,
        resumption: resumption.clone(),
    };

    // TODO: make the expiration configurable through settings
    // let the ticket expire in 30 seconds
    redis_conn
        .set_ex(ticket.redis_key(), &ticket_data, 30)
        .await
        .map_err(|e| {
            log::error!("Unable to store ticket in redis, {}", e);
            StartError::Internal
        })?;

    Ok(StartResponse { ticket, resumption })
}
