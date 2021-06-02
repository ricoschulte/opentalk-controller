//! Room related API structs and Endpoints
//!
//! The defined structs are exposed to the REST API and will be serialized/deserialized. Similar
//! structs are defined in the Database module [`k3k_controller::db`] for database operations.
use crate::api::v1::ApiError;
use crate::db::rooms as db_rooms;
use crate::db::users::User;
use crate::db::DbInterface;
use crate::oidc::OidcContext;
use actix_web::web::{Data, Json, Path, ReqData};
use actix_web::{get, post, put, web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A Room
///
/// Contains all room information. Is only be accessible to the owner and users with
/// appropriate permissions.
#[derive(Debug, Serialize)]
pub struct Room {
    pub uuid: Uuid,
    pub owner: i64,
    pub password: String,
    pub wait_for_moderator: bool,
    pub listen_only: bool,
}

/// Public room details
///
/// Contains general public information about a room.
#[derive(Debug, Serialize)]
pub struct RoomDetails {
    pub uuid: Uuid,
    pub owner: i64,
    pub wait_for_moderator: bool,
    pub listen_only: bool,
}

/// API request parameters to create a new room
#[derive(Debug, Deserialize)]
pub struct NewRoom {
    pub password: String,
    pub wait_for_moderator: bool,
    pub listen_only: bool,
}

/// API request parameters to modify a room.
#[derive(Debug, Deserialize)]
pub struct ModifyRoom {
    pub password: Option<String>,
    pub wait_for_moderator: Option<bool>,
    pub listen_only: Option<bool>,
}

/// API Endpoint *GET /rooms*
///
/// Returns a JSON array of all owned rooms as [`Room`]
#[get("/rooms")]
pub async fn owned(
    db_ctx: Data<DbInterface>,
    current_user: ReqData<User>,
) -> Result<Json<Vec<Room>>, ApiError> {
    let rooms = web::block(move || -> Result<Vec<db_rooms::Room>, ApiError> {
        Ok(db_ctx.get_owned_rooms(&current_user)?)
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on GET /rooms - {}", e);
        ApiError::Internal
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
) -> Result<Json<Room>, ApiError> {
    let new_room = new_room.into_inner();

    let db_room = web::block(move || -> Result<db_rooms::Room, ApiError> {
        let new_room = db_rooms::NewRoom {
            uuid: uuid::Uuid::new_v4(),
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
        ApiError::Internal
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
    room_uuid: Path<Uuid>,
    modify_room: Json<ModifyRoom>,
) -> Result<Json<Room>, ApiError> {
    // TODO: dont allow empty ModifyRoom structs, will result in diesel error (use validator crate)
    let room_uuid = room_uuid.into_inner();
    let modify_room = modify_room.into_inner();

    let db_room = web::block(move || {
        let room = db_ctx.get_room_by_uuid(&room_uuid)?;

        match room {
            None => Err(ApiError::NotFound),
            Some(room) => {
                // TODO: check user permissions when implemented
                if room.owner != current_user.id {
                    return Err(ApiError::InsufficientPermission);
                }

                let change_room = db_rooms::ModifyRoom {
                    owner: None, // Owner can currently not be changed
                    password: modify_room.password,
                    wait_for_moderator: modify_room.wait_for_moderator,
                    listen_only: modify_room.listen_only,
                };

                Ok(db_ctx.modify_room_by_uuid(&room_uuid, change_room)?)
            }
        }
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on PUT /rooms{{room_id}} - {}", e);
        ApiError::Internal
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
    room_uuid: Path<Uuid>,
) -> Result<Json<RoomDetails>, ApiError> {
    let room_uuid = room_uuid.into_inner();

    let db_room = web::block(move || {
        let room = db_ctx.get_room_by_uuid(&room_uuid)?;

        match room {
            None => Err(ApiError::NotFound),
            Some(room) => Ok(room),
        }
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on GET /rooms{{room_id}} - {}", e);
        ApiError::Internal
    })??;

    let room_details = RoomDetails {
        uuid: db_room.uuid,
        owner: db_room.owner,
        wait_for_moderator: db_room.wait_for_moderator,
        listen_only: db_room.listen_only,
    };

    Ok(Json(room_details))
}

/// API Endpoint *GET /rooms/{room_uuid}/start*
///
/// TODO: Implement redirection
#[get("/rooms/{room_uuid}/start")]
pub async fn start(
    _db_ctx: Data<DbInterface>,
    _oidc_ctx: Data<OidcContext>,
    _request: HttpRequest,
    _room_id: Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    Ok(HttpResponse::NotImplemented().finish())
}
