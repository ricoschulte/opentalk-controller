//! Room related API structs and Endpoints
//!
//! The defined structs are exposed to the REST API and will be serialized/deserialized. Similar
//! structs are defined in the Database crate [`db_storage`] for database operations.

use super::response::error::{ApiError, ValidationErrorEntry};
use super::response::{NoContent, CODE_INVALID_VALUE};
use crate::api::signaling::prelude::*;
use crate::api::signaling::resumption::{ResumptionData, ResumptionToken};
use crate::api::signaling::ticket::{TicketData, TicketToken};
use crate::api::v1::{ApiResponse, PagePaginationQuery};
use crate::api::Participant;
use actix_web::web::{self, Data, Json, Path, ReqData};
use actix_web::{delete, get, post, put};
use anyhow::Context;
use controller_shared::ParticipantId;
use database::{Db, OptionalExt};
use db_storage::invites::{Invite, InviteCodeId};
use db_storage::rooms::{self as db_rooms, Room, RoomId};
use db_storage::sip_configs::{NewSipConfig, SipConfig, SipId, SipPassword};
use db_storage::users::{User, UserId};
use kustos::policies_builder::{GrantingAccess, PoliciesBuilder};
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
pub struct RoomResource {
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
        Err(ValidationError::new("empty"))
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
) -> Result<ApiResponse<Vec<RoomResource>>, ApiError> {
    let current_user = current_user.into_inner();
    let PagePaginationQuery { per_page, page } = pagination.into_inner();

    let accessible_rooms: kustos::AccessibleResources<RoomId> = authz
        .get_accessible_resources_for_user(current_user.id, AccessMethod::Get)
        .await?;

    let (rooms, room_count) = crate::block(move || {
        let conn = db.get_conn()?;

        match accessible_rooms {
            kustos::AccessibleResources::All => Room::get_all_paginated(&conn, per_page, page),
            kustos::AccessibleResources::List(list) => {
                Room::get_by_ids_paginated(&conn, &list, per_page, page)
            }
        }
    })
    .await??;

    let rooms = rooms
        .into_iter()
        .map(|db_room| RoomResource {
            uuid: db_room.id,
            owner: db_room.created_by,
            password: db_room.password,
            wait_for_moderator: db_room.wait_for_moderator,
            listen_only: db_room.listen_only,
        })
        .collect::<Vec<RoomResource>>();

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
) -> Result<Json<RoomResource>, ApiError> {
    let room_parameters = room_parameters.into_inner();

    room_parameters.validate()?;

    let current_user_id = current_user.id;

    let db_room = crate::block(move || -> database::Result<_> {
        let conn = db.get_conn()?;

        let new_room = db_rooms::NewRoom {
            created_by: current_user_id,
            password: room_parameters.password,
            wait_for_moderator: room_parameters.wait_for_moderator,
            listen_only: room_parameters.listen_only,
        };

        let room = new_room.insert(&conn)?;

        if room_parameters.enable_sip {
            NewSipConfig::new(room.id, false).insert(&conn)?;
        }

        Ok(room)
    })
    .await??;

    let room = RoomResource {
        uuid: db_room.id,
        owner: db_room.created_by,
        password: db_room.password,
        wait_for_moderator: db_room.wait_for_moderator,
        listen_only: db_room.listen_only,
    };

    let policies = PoliciesBuilder::new()
        .grant_user_access(current_user.id)
        .room_read_access(room.uuid)
        .room_write_access(room.uuid)
        .finish();

    authz.add_policies(policies).await?;

    Ok(Json(room))
}

/// API Endpoint *PUT /rooms/{room_id}*
///
/// Uses the provided [`ModifyRoom`] to modify a specified room.
/// Returns the modified [`Room`]
#[put("/rooms/{room_id}")]
pub async fn modify(
    db: Data<Db>,
    room_id: Path<RoomId>,
    modify_room: Json<ModifyRoom>,
) -> Result<Json<RoomResource>, ApiError> {
    let room_id = room_id.into_inner();
    let modify_room = modify_room.into_inner();

    modify_room.validate()?;

    let db_room = crate::block(move || {
        let conn = db.get_conn()?;

        let changeset = db_rooms::UpdateRoom {
            password: modify_room.password,
            wait_for_moderator: modify_room.wait_for_moderator,
            listen_only: modify_room.listen_only,
        };

        changeset.apply(&conn, room_id)
    })
    .await??;

    let room = RoomResource {
        uuid: db_room.id,
        owner: db_room.created_by,
        password: db_room.password,
        wait_for_moderator: db_room.wait_for_moderator,
        listen_only: db_room.listen_only,
    };

    Ok(Json(room))
}

/// API Endpoint *DELETE /rooms/{room_id}*
///
/// Deletes the room and owned resources.
#[delete("/rooms/{room_id}")]
pub async fn delete(
    db: Data<Db>,
    room_id: Path<RoomId>,
    authz: Data<Authz>,
) -> Result<NoContent, ApiError> {
    let room_id = room_id.into_inner();
    let room_path = format!("/rooms/{}", room_id);

    crate::block(move || {
        let conn = db.get_conn()?;

        Room::delete_by_id(&conn, room_id)
    })
    .await??;

    if !authz
        .remove_explicit_resource_permissions(room_path.clone())
        .await?
    {
        log::error!("Failed to delete permissions for {}", room_path);
    }

    Ok(NoContent {})
}

/// API Endpoint *GET /rooms/{room_id}*
///
/// Returns the specified Room as [`RoomDetails`].
#[get("/rooms/{room_id}")]
pub async fn get(db: Data<Db>, room_id: Path<RoomId>) -> Result<Json<RoomDetails>, ApiError> {
    let room_id = room_id.into_inner();

    let db_room = crate::block(move || {
        let conn = db.get_conn()?;

        Room::get(&conn, room_id)
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

/// The JSON body expected when making a *POST /rooms/{room_id}/start*
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

#[derive(Debug)]
pub enum StartRoomError {
    WrongRoomPassword,
    InvalidCredentials,
    NoBreakoutRooms,
    InvalidBreakoutRoomId,
    BannedFromRoom,
}

impl From<StartRoomError> for ApiError {
    fn from(start_room_error: StartRoomError) -> Self {
        match start_room_error {
            StartRoomError::WrongRoomPassword => ApiError::unauthorized()
                .with_code("wrong_room_password")
                .with_message("The provided password does not match the rooms password"),

            StartRoomError::InvalidCredentials => {
                ApiError::unauthorized().with_code("invalid_credentials")
            }

            StartRoomError::NoBreakoutRooms => ApiError::bad_request()
                .with_code("no_breakout_rooms")
                .with_message("The requested room has no breakout rooms"),

            StartRoomError::InvalidBreakoutRoomId => ApiError::bad_request()
                .with_code("invalid_breakout_room_id")
                .with_message("The provided breakout room ID is invalid"),

            StartRoomError::BannedFromRoom => ApiError::forbidden()
                .with_code("banned_from_room")
                .with_message("This user has been banned from entering this room"),
        }
    }
}

/// API Endpoint *POST /rooms/{room_id}/start*
///
/// This endpoint has to be called in order to get a room ticket. When joining a room, the ticket
/// must be provided as a `Sec-WebSocket-Protocol` header field when starting the WebSocket
/// connection.
///
/// When the requested room has a password set, the requester has to provide the correct password
/// through the [`StartRequest`] JSON in the requests body. When the room has no password set,
/// the provided password will be ignored.
///
/// Returns a [`StartResponse`] containing the ticket for the specified room.
///
/// # Errors
///
/// Returns [`StartRoomError::WrongRoomPassword`] when the provided password is wrong
/// Returns [`StartRoomError::NoBreakoutRooms`]  when no breakout rooms are configured but were provided
/// Returns [`StartRoomError::InvalidBreakoutRoomId`]  when the provided breakout room id is invalid     
#[post("/rooms/{room_id}/start")]
pub async fn start(
    db: Data<Db>,
    redis_ctx: Data<ConnectionManager>,
    current_user: ReqData<User>,
    room_id: Path<RoomId>,
    request: Json<StartRequest>,
) -> Result<Json<StartResponse>, ApiError> {
    let request = request.into_inner();
    let room_id = room_id.into_inner();

    let room = crate::block(move || {
        let conn = db.get_conn()?;

        Room::get(&conn, room_id)
    })
    .await??;

    if !room.password.is_empty() {
        if let Some(pw) = &request.password {
            if pw != &room.password {
                return Err(StartRoomError::WrongRoomPassword.into());
            }
        } else {
            return Err(StartRoomError::WrongRoomPassword.into());
        }
    }

    let mut redis_conn = (**redis_ctx).clone();

    // check if user is banned from room
    if moderation::storage::is_banned(&mut redis_conn, room.id, current_user.id).await? {
        return Err(StartRoomError::BannedFromRoom.into());
    }

    if let Some(breakout_room) = request.breakout_room {
        let config = breakout::storage::get_config(&mut redis_conn, room.id).await?;

        if let Some(config) = config {
            if !config.is_valid_id(breakout_room) {
                return Err(StartRoomError::InvalidBreakoutRoomId.into());
            }
        } else {
            return Err(StartRoomError::NoBreakoutRooms.into());
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

/// The JSON body expected when making a *POST /rooms/{room_id}/start_invited*
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
) -> Result<ApiResponse<StartResponse>, ApiError> {
    let request = request.into_inner();
    let room_id = room_id.into_inner();

    let invite_code_as_uuid = uuid::Uuid::from_str(&request.invite_code).map_err(|_| {
        ApiError::unprocessable_entities([ValidationErrorEntry::new(
            "invite_code",
            CODE_INVALID_VALUE,
            Some("Bad invite code format"),
        )])
    })?;

    let room = crate::block(move || -> Result<db_rooms::Room, ApiError> {
        let conn = db.get_conn()?;

        let invite = Invite::get(&conn, InviteCodeId::from(invite_code_as_uuid))?;

        if !invite.active {
            return Err(ApiError::not_found());
        }

        if invite.room != room_id {
            return Err(ApiError::bad_request().with_message("Room id mismatch"));
        }

        let room = Room::get(&conn, invite.room)?;

        Ok(room)
    })
    .await??;

    if !room.password.is_empty() {
        if let Some(pw) = &request.password {
            if pw != &room.password {
                return Err(StartRoomError::WrongRoomPassword.into());
            }
        } else {
            return Err(StartRoomError::WrongRoomPassword.into());
        }
    }

    let mut redis_conn = (**redis_ctx).clone();

    if let Some(breakout_room) = request.breakout_room {
        let config = breakout::storage::get_config(&mut redis_conn, room.id).await?;

        if let Some(config) = config {
            if !config.is_valid_id(breakout_room) {
                return Err(StartRoomError::InvalidBreakoutRoomId.into());
            }
        } else {
            return Err(StartRoomError::NoBreakoutRooms.into());
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
/// Get a [`StartResponse`] for a new sip connection to a room. The requester has to provide
/// a valid [`SipId`] & [`SipPassword`] via the [`SipStartRequest`]
///
/// # Errors
///
/// Returns [`StartRoomError::InvalidCredentials`] when the provided [`SipId`] or [`SipPassword`] is wrong
#[post("/rooms/sip/start")]
pub async fn sip_start(
    db: Data<Db>,
    redis_ctx: Data<ConnectionManager>,
    request: Json<SipStartRequest>,
) -> Result<ApiResponse<StartResponse>, ApiError> {
    let mut redis_conn = (**redis_ctx).clone();
    let request = request.into_inner();

    request.sip_id.validate()?;

    request.password.validate()?;

    let room_id = crate::block(move || -> Result<RoomId, ApiError> {
        let conn = db.get_conn()?;

        if let Some(sip_config) = SipConfig::get(&conn, request.sip_id).optional()? {
            if sip_config.password == request.password {
                Ok(sip_config.room)
            } else {
                Err(StartRoomError::InvalidCredentials.into())
            }
        } else {
            Err(StartRoomError::InvalidCredentials.into())
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
) -> Result<StartResponse, ApiError> {
    // Get participant id, check resumption token if it exists, if not generate random one
    let participant_id = if let Some(resumption) = resumption {
        let resumption_redis_key = resumption.into_redis_key();

        // Check for resumption data behind resumption token
        let resumption_data: Option<ResumptionData> =
            redis_conn.get(&resumption_redis_key).await.map_err(|e| {
                log::error!("Failed to fetch resumption token from redis, {}", e);
                ApiError::internal()
            })?;

        // If redis returned None generate random id, else check if request matches resumption data
        if let Some(data) = resumption_data {
            if data.room == room && data.participant == participant {
                let in_use =
                    control::storage::participant_id_in_use(redis_conn, data.participant_id)
                        .await?;

                if in_use {
                    return Err(ApiError::bad_request().with_message(
                        "the session of the given resumption token is still running",
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
            ApiError::internal()
        })?;

    Ok(StartResponse { ticket, resumption })
}

pub trait RoomsPoliciesBuilderExt {
    fn room_read_access(self, room_id: RoomId) -> Self;
    fn room_write_access(self, room_id: RoomId) -> Self;
}

impl<T> RoomsPoliciesBuilderExt for PoliciesBuilder<GrantingAccess<T>>
where
    T: IsSubject + Clone,
{
    fn room_read_access(self, room_id: RoomId) -> Self {
        self.add_resource(room_id.resource_id(), [AccessMethod::Get])
            .add_resource(
                room_id.resource_id().with_suffix("/invites"),
                [AccessMethod::Get],
            )
            .add_resource(
                room_id.resource_id().with_suffix("/start"),
                [AccessMethod::Post],
            )
    }

    fn room_write_access(self, room_id: RoomId) -> Self {
        self.add_resource(
            room_id.resource_id(),
            [AccessMethod::Put, AccessMethod::Delete],
        )
        .add_resource(
            room_id.resource_id().with_suffix("/invites"),
            [AccessMethod::Post],
        )
    }
}
