//! Room related API structs and Endpoints
//!
//! The defined structs are exposed to the REST API and will be serialized/deserialized. Similar
//! structs are defined in the Database module [`crate::db`] for database operations.

use crate::api::v1::{ApiError, DefaultApiError};
use crate::db::rooms::{self as db_rooms, RoomId};
use crate::db::users::{User, UserId};
use crate::db::DbInterface;
use actix_web::web::{Data, Json, Path, ReqData};
use actix_web::{get, post, put, web};
use displaydoc::Display;
use rand::Rng;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
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
    db_ctx: Data<DbInterface>,
    current_user: ReqData<User>,
) -> Result<Json<Vec<Room>>, DefaultApiError> {
    let rooms = web::block(move || -> Result<Vec<db_rooms::Room>, DefaultApiError> {
        Ok(db_ctx.get_owned_rooms(&current_user)?)
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on GET /rooms - {}", e);
        DefaultApiError::Internal
    })??;

    let rooms = rooms
        .into_iter()
        .map(|db_room| Room {
            uuid: db_room.uuid,
            owner: db_room.owner,
            password: db_room.password,
            wait_for_moderator: db_room.wait_for_moderator,
            listen_only: db_room.listen_only,
        })
        .collect::<Vec<Room>>();

    Ok(Json(rooms))
}

/// API Endpoint *POST /rooms*
///
/// Uses the provided [`NewRoom`] to create a new room.
/// Returns the created [`Room`].
#[post("/rooms")]
pub async fn new(
    db_ctx: Data<DbInterface>,
    current_user: ReqData<User>,
    new_room: Json<NewRoom>,
) -> Result<Json<Room>, DefaultApiError> {
    let new_room = new_room.into_inner();

    if let Err(e) = new_room.validate() {
        log::warn!("API new room validation error {}", e);
        return Err(DefaultApiError::ValidationFailed);
    }

    let db_room = web::block(move || -> Result<db_rooms::Room, DefaultApiError> {
        let new_room = db_rooms::NewRoom {
            uuid: RoomId::from(uuid::Uuid::new_v4()),
            owner: current_user.id,
            password: new_room.password,
            wait_for_moderator: new_room.wait_for_moderator,
            listen_only: new_room.listen_only,
        };

        Ok(db_ctx.new_room(new_room)?)
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on POST /rooms - {}", e);
        DefaultApiError::Internal
    })??;

    let room = Room {
        uuid: db_room.uuid,
        owner: db_room.owner,
        password: db_room.password,
        wait_for_moderator: db_room.wait_for_moderator,
        listen_only: db_room.listen_only,
    };

    Ok(Json(room))
}

/// API Endpoint *PUT /rooms/{room_uuid}*
///
/// Uses the provided [`ModifyRoom`] to modify a specified room.
/// Returns the modified [`Room`]
#[put("/rooms/{room_uuid}")]
pub async fn modify(
    db_ctx: Data<DbInterface>,
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

    let db_room = web::block(move || {
        let room = db_ctx.get_room(room_id)?;

        match room {
            None => Err(DefaultApiError::NotFound),
            Some(room) => {
                // TODO: check user permissions when implemented
                if room.owner != current_user.id {
                    return Err(DefaultApiError::InsufficientPermission);
                }

                let change_room = db_rooms::ModifyRoom {
                    owner: None, // Owner can currently not be changed
                    password: modify_room.password,
                    wait_for_moderator: modify_room.wait_for_moderator,
                    listen_only: modify_room.listen_only,
                };

                Ok(db_ctx.modify_room(room_id, change_room)?)
            }
        }
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on PUT /rooms{{room_id}} - {}", e);
        DefaultApiError::Internal
    })??;

    let room = Room {
        uuid: db_room.uuid,
        owner: db_room.owner,
        password: db_room.password,
        wait_for_moderator: db_room.wait_for_moderator,
        listen_only: db_room.listen_only,
    };

    Ok(Json(room))
}

/// API Endpoint *GET /rooms/{room_uuid}*
///
/// Returns the specified Room as [`RoomDetails`].
#[get("/rooms/{room_uuid}")]
pub async fn get(
    db_ctx: Data<DbInterface>,
    room_id: Path<RoomId>,
) -> Result<Json<RoomDetails>, DefaultApiError> {
    let room_id = room_id.into_inner();

    let db_room = web::block(move || {
        let room = db_ctx.get_room(room_id)?;

        match room {
            None => Err(DefaultApiError::NotFound),
            Some(room) => Ok(room),
        }
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on GET /rooms{{room_id}} - {}", e);
        DefaultApiError::Internal
    })??;

    let room_details = RoomDetails {
        uuid: db_room.uuid,
        owner: db_room.owner,
        wait_for_moderator: db_room.wait_for_moderator,
        listen_only: db_room.listen_only,
    };

    Ok(Json(room_details))
}

/// The JSON body expected when making a *POST /rooms/{room_uuid}/start*
#[derive(Debug, Deserialize)]
pub struct StartRequest {
    password: Option<String>,
}

#[derive(Display, Validate, Debug, Clone, Serialize)]
/// k3k-signaling:ticket={ticket}
#[ignore_extra_doc_attributes]
/// The ticket for a room used at /signaling to start a WebSocket connection.
///
/// The Display implementation represents the redis key for the underlying [`TicketData`]
pub struct Ticket {
    #[validate(length(min = 64, max = 64))]
    pub ticket: String,
}

impl_to_redis_args!(&Ticket);

/// Data stored behind the [`Ticket`] key.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TicketData {
    pub user: UserId,
    pub room: RoomId,
}

impl_from_redis_value_de!(TicketData);
impl_to_redis_args_se!(&TicketData);

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StartRoomError {
    WrongRoomPassword,
}

type StartError = ApiError<StartRoomError>;

/// API Endpoint *POST /rooms/{room_uuid}/start*
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
#[post("/rooms/{room_uuid}/start")]
pub async fn start(
    db_ctx: Data<DbInterface>,
    redis_ctx: Data<ConnectionManager>,
    current_user: ReqData<User>,
    room_id: Path<RoomId>,
    request: Json<StartRequest>,
) -> Result<Json<Ticket>, StartError> {
    let room_id = room_id.into_inner();

    let room = web::block(move || -> Result<db_rooms::Room, StartError> {
        let room = db_ctx.get_room(room_id)?.ok_or(StartError::NotFound)?;

        Ok(room)
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on GET /rooms/{{room_uuid}}/start - {}", e);
        StartError::Internal
    })??;

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

    let ticket = rand::thread_rng()
        .sample_iter(rand::distributions::Alphanumeric)
        .take(64)
        .map(char::from)
        .collect::<String>();

    let ticket = Ticket { ticket };

    let ticket_data = TicketData {
        user: current_user.id,
        room: room_id,
    };

    let mut redis_conn = (**redis_ctx).clone();

    // TODO: make the expiration configurable through settings
    // let the ticket expire in 30 seconds
    redis_conn
        .set_ex(&ticket, &ticket_data, 30)
        .await
        .map_err(|e| {
            log::error!("Unable to store ticket in redis, {}", e);
            StartError::Internal
        })?;

    Ok(Json(ticket))
}
