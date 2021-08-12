//! User related API structs and Endpoints
//!
//! The defined structs are exposed to the REST API and will be serialized/deserialized. Similar
//! structs are defined in the Database module [`crate::db`] for database operations.

use crate::api::v1::DefaultApiError;
use crate::db::users::User;
use crate::db::users::{self as db_users, UserId};
use crate::db::DbInterface;
use actix_web::web::{Data, Json, Path, ReqData};
use actix_web::{get, put};
use serde::{Deserialize, Serialize};
use validator::{Validate, ValidationError};

/// Public user details.
///
/// Contains general "public" information about a user. Is accessible to all other users.
#[derive(Debug, Serialize)]
pub struct UserDetails {
    pub id: UserId,
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
    pub id: UserId,
    pub email: String,
    pub title: String,
    pub firstname: String,
    pub lastname: String,
    pub theme: String,
    pub language: String,
}

/// Used to modify user settings
#[derive(Debug, Validate, Deserialize)]
#[validate(schema(function = "disallow_empty"))]
pub struct ModifyUser {
    #[validate(length(max = 255))]
    pub title: Option<String>,
    #[validate(length(max = 255))]
    pub theme: Option<String>,
    #[validate(length(max = 35))]
    pub language: Option<String>,
}

fn disallow_empty(modify_user: &ModifyUser) -> Result<(), ValidationError> {
    let ModifyUser {
        title,
        theme,
        language,
    } = modify_user;

    if title.is_none() && theme.is_none() && language.is_none() {
        Err(ValidationError::new("ModifyUser has no set fields"))
    } else {
        Ok(())
    }
}

/// API Endpoint *GET /users*
///
/// Returns a JSON array of all database users as [`UserDetails`]
#[get("/users")]
pub async fn all(db_ctx: Data<DbInterface>) -> Result<Json<Vec<UserDetails>>, DefaultApiError> {
    let db_users = crate::block(move || -> Result<Vec<db_users::User>, DefaultApiError> {
        Ok(db_ctx.get_users()?)
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on GET /users - {}", e);
        DefaultApiError::Internal
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
) -> Result<Json<UserProfile>, DefaultApiError> {
    let modify_user = modify_user.into_inner();

    if let Err(e) = modify_user.validate() {
        log::warn!("API modify user validation error {}", e);
        return Err(DefaultApiError::ValidationFailed);
    }

    let db_user = crate::block(move || -> Result<db_users::User, DefaultApiError> {
        let modify_user = db_users::ModifyUser {
            title: modify_user.title,
            theme: modify_user.theme,
            language: modify_user.language,
            id_token_exp: None,
        };

        let modified_user = db_ctx.modify_user(current_user.oidc_uuid, modify_user, None)?;

        Ok(modified_user.user)
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on PUT /users - {}", e);
        DefaultApiError::Internal
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
) -> Result<Json<UserProfile>, DefaultApiError> {
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
    user_id: Path<UserId>,
) -> Result<Json<UserDetails>, DefaultApiError> {
    let db_user = crate::block(move || -> Result<Option<db_users::User>, DefaultApiError> {
        Ok(db_ctx.get_user_by_id(user_id.into_inner())?)
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on GET /users/me - {}", e);
        DefaultApiError::Internal
    })??;

    match db_user {
        None => Err(DefaultApiError::NotFound),
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
