//! User related API structs and Endpoints
//!
//! The defined structs are exposed to the REST API and will be serialized/deserialized. Similar
//! structs are defined in the Database module [`k3k_controller::db`] for database operations.
use crate::api::v1::ApiError;
use crate::db::users as db_users;
use crate::db::users::User;
use crate::db::DbInterface;
use actix_web::web::{Data, Json, Path, ReqData};
use actix_web::{get, put, web};
use serde::{Deserialize, Serialize};

/// Public user details.
///
/// Contains general "public" information about a user. Is accessible to all other users.
#[derive(Debug, Serialize)]
pub struct UserDetails {
    pub id: i64,
    pub email: String,
    pub title: String,
    pub firstname: String,
    pub lastname: String,
}

/// Private user profile.
///
/// Similar to [`UserDetails`], but contains additional "private" information about a user.
/// Is only accessible to the user himself.
/// Is used on */users/me* endpoints.
#[derive(Debug, Serialize)]
pub struct UserProfile {
    pub id: i64,
    pub email: String,
    pub title: String,
    pub firstname: String,
    pub lastname: String,
    pub theme: String,
    pub language: String,
}

/// Used to modify user settings
#[derive(Debug, Deserialize)]
pub struct ModifyUser {
    pub title: Option<String>,
    pub theme: Option<String>,
    pub language: Option<String>,
}

/// API Endpoint *GET /users*
///
/// Returns a JSON array of all database users as [`UserDetails`]
#[get("/users")]
pub async fn all(db_ctx: Data<DbInterface>) -> Result<Json<Vec<UserDetails>>, ApiError> {
    let db_users =
        web::block(move || -> Result<Vec<db_users::User>, ApiError> { Ok(db_ctx.get_users()?) })
            .await
            .map_err(|e| {
                log::error!("BlockingError on GET /users - {}", e);
                ApiError::Internal
            })??;

    let users = db_users
        .into_iter()
        .map(|db_user| UserDetails {
            id: db_user.id,
            email: db_user.email,
            title: db_user.title,
            firstname: db_user.firstname,
            lastname: db_user.lastname,
        })
        .collect::<Vec<UserDetails>>();

    Ok(Json(users))
}

/// API Endpoint *PUT /users/me*
///
/// Serializes [`ModifyUser`] and modifies the user settings of the requesting user accordingly.
/// Returns the [`UserProfile`] of the affected user.
#[put("/users/me")]
pub async fn set_current_user_profile(
    db_ctx: Data<DbInterface>,
    current_user: ReqData<User>,
    modify_user: Json<ModifyUser>,
) -> Result<Json<UserProfile>, ApiError> {
    let modify_user = modify_user.into_inner();

    let db_user = web::block(move || -> Result<db_users::User, ApiError> {
        let modify_user = db_users::ModifyUser {
            title: modify_user.title,
            theme: modify_user.theme,
            language: modify_user.language,
            id_token_exp: None,
        };

        Ok(db_ctx.modify_user(current_user.oidc_uuid, modify_user)?)
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on PUT /users - {}", e);
        ApiError::Internal
    })??;

    let user_profile = UserProfile {
        id: db_user.id,
        email: db_user.email,
        title: db_user.title,
        firstname: db_user.firstname,
        lastname: db_user.lastname,
        theme: db_user.theme,
        language: db_user.language,
    };

    Ok(Json(user_profile))
}

/// API Endpoint *GET /users/me*
///
/// Returns the ['UserProfile'] of the requesting user.
#[get("/users/me")]
pub async fn current_user_profile(
    current_user: ReqData<User>,
) -> Result<Json<UserProfile>, ApiError> {
    let current_user = current_user.into_inner();

    let user_profile = UserProfile {
        id: current_user.id,
        email: current_user.email,
        title: current_user.title,
        firstname: current_user.firstname,
        lastname: current_user.lastname,
        theme: current_user.theme,
        language: current_user.language,
    };

    Ok(Json(user_profile))
}

/// API Endpoint *GET /users/{user_id}*
///
/// Returns ['UserDetails'] of the specified user
#[get("/users/{user_id}")]
pub async fn user_details(
    db_ctx: Data<DbInterface>,
    user_id: Path<i64>,
) -> Result<Json<UserDetails>, ApiError> {
    let db_user = web::block(move || -> Result<Option<db_users::User>, ApiError> {
        Ok(db_ctx.get_user_by_id(user_id.into_inner())?)
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on GET /users/me - {}", e);
        ApiError::Internal
    })??;

    match db_user {
        None => Err(ApiError::NotFound),
        Some(db_user) => {
            let user_details = UserDetails {
                id: db_user.id,
                email: db_user.email,
                title: db_user.title,
                firstname: db_user.firstname,
                lastname: db_user.lastname,
            };

            Ok(Json(user_details))
        }
    }
}
