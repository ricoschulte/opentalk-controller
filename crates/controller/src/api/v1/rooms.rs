// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! Room related API structs and Endpoints
//!
//! The defined structs are exposed to the REST API and will be serialized/deserialized. Similar
//! structs are defined in the Database crate [`db_storage`] for database operations.

use super::response::error::{ApiError, ValidationErrorEntry};
use super::response::{NoContent, CODE_INVALID_VALUE};
use super::users::PublicUserProfile;
use crate::api::signaling::prelude::*;
use crate::api::signaling::ticket::{start_or_continue_signaling_session, TicketToken};
use crate::api::v1::tariffs::TariffResource;
use crate::api::v1::{ApiResponse, PagePaginationQuery};
use crate::api::Participant;
use crate::redis_wrapper::RedisConnection;
use crate::settings::SharedSettingsActix;
use actix_web::web::{self, Data, Json, Path, ReqData};
use actix_web::{delete, get, patch, post};
use chrono::{DateTime, Utc};
use database::Db;
use db_storage::invites::Invite;
use db_storage::rooms::{self as db_rooms, Room};
use db_storage::sip_configs::NewSipConfig;
use db_storage::users::User;
use kustos::policies_builder::{GrantingAccess, PoliciesBuilder};
use kustos::prelude::*;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use types::core::{BreakoutRoomId, InviteCodeId, ResumptionToken, RoomId};
use validator::Validate;

/// A Room
///
/// Contains all room information. Is only be accessible to the owner and users with
/// appropriate permissions.
#[derive(Debug, Serialize)]
pub struct RoomResource {
    pub id: RoomId,
    pub created_by: PublicUserProfile,
    pub created_at: DateTime<Utc>,
    pub password: Option<String>,
    pub waiting_room: bool,
}

/// API Endpoint *GET /rooms*
///
/// Returns a JSON array of all accessible rooms as [`Room`]
#[get("/rooms")]
pub async fn accessible(
    settings: SharedSettingsActix,
    db: Data<Db>,
    current_user: ReqData<User>,
    pagination: web::Query<PagePaginationQuery>,
    authz: Data<Authz>,
) -> Result<ApiResponse<Vec<RoomResource>>, ApiError> {
    let settings = settings.load();
    let current_user = current_user.into_inner();
    let PagePaginationQuery { per_page, page } = pagination.into_inner();

    let accessible_rooms: kustos::AccessibleResources<RoomId> = authz
        .get_accessible_resources_for_user(current_user.id, AccessMethod::Get)
        .await?;

    let (rooms, room_count) = crate::block(move || {
        let mut conn = db.get_conn()?;

        match accessible_rooms {
            kustos::AccessibleResources::All => {
                Room::get_all_with_creator_paginated(&mut conn, per_page, page)
            }
            kustos::AccessibleResources::List(list) => {
                Room::get_by_ids_with_creator_paginated(&mut conn, &list, per_page, page)
            }
        }
    })
    .await??;

    let rooms = rooms
        .into_iter()
        .map(|(room, user)| RoomResource {
            id: room.id,
            created_by: PublicUserProfile::from_db(&settings, user),
            created_at: room.created_at,
            password: room.password,
            waiting_room: room.waiting_room,
        })
        .collect::<Vec<RoomResource>>();

    Ok(ApiResponse::new(rooms).with_page_pagination(per_page, page, room_count))
}

/// API request parameters to create a new room
#[derive(Debug, Validate, Deserialize)]
pub struct PostRoomsBody {
    #[validate(length(min = 1, max = 255))]
    pub password: Option<String>,
    /// Enable/Disable sip for this room; defaults to false when not set
    #[serde(default)]
    pub enable_sip: bool,
    #[serde(default)]
    pub waiting_room: bool,
}

/// API Endpoint *POST /rooms*
///
/// Uses the provided [`PostRoomsBody`] to create a new room.
/// Returns the created [`RoomResource`].
#[post("/rooms")]
pub async fn new(
    settings: SharedSettingsActix,
    db: Data<Db>,
    authz: Data<Authz>,
    current_user: ReqData<User>,
    body: Json<PostRoomsBody>,
) -> Result<Json<RoomResource>, ApiError> {
    let settings = settings.load();
    let current_user = current_user.into_inner();
    let room_parameters = body.into_inner();

    room_parameters.validate()?;

    let current_user_id = current_user.id;

    let room = crate::block(move || -> database::Result<_> {
        let mut conn = db.get_conn()?;

        let new_room = db_rooms::NewRoom {
            created_by: current_user_id,
            password: room_parameters.password,
            waiting_room: room_parameters.waiting_room,
            tenant_id: current_user.tenant_id,
        };

        let room = new_room.insert(&mut conn)?;

        if room_parameters.enable_sip {
            NewSipConfig::new(room.id, false).insert(&mut conn)?;
        }

        Ok(room)
    })
    .await??;

    let room_resource = RoomResource {
        id: room.id,
        created_by: PublicUserProfile::from_db(&settings, current_user),
        created_at: room.created_at,
        password: room.password,
        waiting_room: room.waiting_room,
    };

    let policies = PoliciesBuilder::new()
        .grant_user_access(current_user_id)
        .room_read_access(room_resource.id)
        .room_write_access(room_resource.id)
        .finish();

    authz.add_policies(policies).await?;

    Ok(Json(room_resource))
}

/// API request parameters to patch a room
#[derive(Debug, Validate, Deserialize)]
pub struct PatchRoomsBody {
    #[validate(length(min = 1, max = 255))]
    #[serde(default, deserialize_with = "super::util::deserialize_some")]
    pub password: Option<Option<String>>,

    pub waiting_room: Option<bool>,
}

/// API Endpoint *PATCH /rooms/{room_id}*
///
/// Uses the provided [`PatchRoomsBody`] to modify a specified room.
/// Returns the modified [`RoomResource`]
#[patch("/rooms/{room_id}")]
pub async fn patch(
    settings: SharedSettingsActix,
    db: Data<Db>,
    current_user: ReqData<User>,
    room_id: Path<RoomId>,
    body: Json<PatchRoomsBody>,
) -> Result<Json<RoomResource>, ApiError> {
    let settings = settings.load();
    let current_user = current_user.into_inner();
    let room_id = room_id.into_inner();
    let modify_room = body.into_inner();

    modify_room.validate()?;

    let room = crate::block(move || {
        let mut conn = db.get_conn()?;

        let changeset = db_rooms::UpdateRoom {
            password: modify_room.password,
            waiting_room: modify_room.waiting_room,
        };

        changeset.apply(&mut conn, room_id)
    })
    .await??;

    let room_resource = RoomResource {
        id: room.id,
        created_by: PublicUserProfile::from_db(&settings, current_user),
        created_at: room.created_at,
        password: room.password,
        waiting_room: room.waiting_room,
    };

    Ok(Json(room_resource))
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

    crate::block(move || {
        let mut conn = db.get_conn()?;

        Room::delete_by_id(&mut conn, room_id)
    })
    .await??;

    let resources = associated_resource_ids(room_id);

    authz.remove_explicit_resources(resources).await?;

    Ok(NoContent)
}

/// API Endpoint *GET /rooms/{room_id}*
///
/// Returns the specified Room as [`RoomResource`].
#[get("/rooms/{room_id}")]
pub async fn get(
    settings: SharedSettingsActix,
    db: Data<Db>,
    room_id: Path<RoomId>,
) -> Result<Json<RoomResource>, ApiError> {
    let settings = settings.load();
    let room_id = room_id.into_inner();

    let (room, created_by) = crate::block(move || {
        let mut conn = db.get_conn()?;

        Room::get_with_user(&mut conn, room_id)
    })
    .await??;

    let room_resource = RoomResource {
        id: room.id,
        created_by: PublicUserProfile::from_db(&settings, created_by),
        created_at: room.created_at,
        password: room.password,
        waiting_room: room.waiting_room,
    };

    Ok(Json(room_resource))
}

#[get("/rooms/{room_id}/tariff")]
pub async fn get_room_tariff(
    db: Data<Db>,
    modules: Data<SignalingModules>,
    room_id: Path<RoomId>,
) -> Result<Json<TariffResource>, ApiError> {
    let room_id = room_id.into_inner();

    let tariff = crate::block(move || {
        let mut conn = db.get_conn()?;

        let room = Room::get(&mut conn, room_id)?;
        room.get_tariff(&mut conn)
    })
    .await??;

    let response = TariffResource::from_tariff(tariff, &modules.get_module_names());

    Ok(Json(response))
}

/// The JSON body expected when making a *POST /rooms/{room_id}/start*
#[derive(Debug, Deserialize)]
pub struct StartRequest {
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
    redis_conn: Data<RedisConnection>,
    current_user: ReqData<User>,
    room_id: Path<RoomId>,
    request: Json<StartRequest>,
) -> Result<Json<StartResponse>, ApiError> {
    let request = request.into_inner();
    let room_id = room_id.into_inner();

    let room = crate::block(move || {
        let mut conn = db.get_conn()?;

        Room::get(&mut conn, room_id)
    })
    .await??;

    let mut redis_conn = (**redis_conn).clone();

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

    let (ticket, resumption) = start_or_continue_signaling_session(
        &mut redis_conn,
        current_user.id.into(),
        room_id,
        request.breakout_room,
        request.resumption,
    )
    .await?;

    Ok(Json(StartResponse { ticket, resumption }))
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
    redis_ctx: Data<RedisConnection>,
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
        let mut conn = db.get_conn()?;

        let invite = Invite::get(&mut conn, InviteCodeId::from(invite_code_as_uuid))?;

        if !invite.active {
            return Err(ApiError::not_found());
        }

        if invite.room != room_id {
            return Err(ApiError::bad_request().with_message("Room id mismatch"));
        }

        let room = Room::get(&mut conn, invite.room)?;

        Ok(room)
    })
    .await??;

    if let Some(password) = &room.password {
        if let Some(pw) = &request.password {
            if pw != password {
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

    let (ticket, resumption) = start_or_continue_signaling_session(
        &mut redis_conn,
        Participant::Guest,
        room_id,
        request.breakout_room,
        request.resumption,
    )
    .await?;

    Ok(ApiResponse::new(StartResponse { ticket, resumption }))
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
            .add_resource(
                room_id.resource_id().with_suffix("/tariff"),
                [AccessMethod::Get],
            )
            .add_resource(
                room_id.resource_id().with_suffix("/assets"),
                [AccessMethod::Get],
            )
            .add_resource(
                room_id.resource_id().with_suffix("/assets/*"),
                [AccessMethod::Get],
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
        .add_resource(
            room_id.resource_id().with_suffix("/invites/*"),
            [AccessMethod::GET, AccessMethod::PUT, AccessMethod::DELETE],
        )
        .add_resource(
            room_id.resource_id().with_suffix("/assets"),
            [AccessMethod::Delete],
        )
        .add_resource(
            room_id.resource_id().with_suffix("/assets/*"),
            [AccessMethod::Delete],
        )
    }
}

pub(crate) fn associated_resource_ids(room_id: RoomId) -> impl IntoIterator<Item = ResourceId> {
    [
        ResourceId::from(format!("/room/{room_id}")),
        ResourceId::from(format!("/room/{room_id}/invites")),
        ResourceId::from(format!("/room/{room_id}/invites/*")),
        ResourceId::from(format!("/room/{room_id}/start")),
        ResourceId::from(format!("/room/{room_id}/tariff")),
    ]
}
