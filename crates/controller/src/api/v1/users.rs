//! User related API structs and Endpoints
//!
//! The defined structs are exposed to the REST API and will be serialized/deserialized. Similar
//! structs are defined in the Database module [`crate::db`] for database operations.

use crate::api::v1::{ApiResponse, DefaultApiError, PagePaginationQuery};
use actix_web::web::{Data, Json, Path, Query, ReqData};
use actix_web::{get, put};
use database::Db;
use db_storage::users::{UpdateUser, User, UserId};
use db_storage::DbUsersEx;
use kustos::prelude::*;
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

impl From<User> for UserDetails {
    fn from(user: User) -> Self {
        UserDetails {
            id: user.id,
            email: user.email,
            title: user.title,
            firstname: user.firstname,
            lastname: user.lastname,
        }
    }
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
pub async fn all(
    db: Data<Db>,
    current_user: ReqData<User>,
    pagination: Query<PagePaginationQuery>,
    authz: Data<Authz>,
) -> Result<ApiResponse<Vec<UserDetails>>, DefaultApiError> {
    let current_user = current_user.into_inner();
    let PagePaginationQuery { per_page, page } = pagination.into_inner();

    let accessible_users: kustos::AccessibleResources<UserId> = authz
        .get_accessible_resources_for_user(current_user.id, AccessMethod::Get)
        .await
        .map_err(|_| DefaultApiError::Internal)?;

    let (users, total_users) =
        crate::block(move || -> Result<(Vec<User>, i64), DefaultApiError> {
            match accessible_users {
                kustos::AccessibleResources::All => Ok(db.get_users_paginated(per_page, page)?),
                kustos::AccessibleResources::List(list) => {
                    Ok(db.get_users_by_ids_paginated(&list, per_page as i64, page as i64)?)
                }
            }
        })
        .await??;

    let users = users
        .into_iter()
        .map(|db_user| UserDetails {
            id: db_user.id,
            email: db_user.email,
            title: db_user.title,
            firstname: db_user.firstname,
            lastname: db_user.lastname,
        })
        .collect::<Vec<UserDetails>>();

    Ok(ApiResponse::new(users).with_page_pagination(per_page, page, total_users))
}

/// API Endpoint *PUT /users/me*
///
/// Serializes [`ModifyUser`] and modifies the user settings of the requesting user accordingly.
/// Returns the [`UserProfile`] of the affected user.
#[put("/users/me")]
pub async fn set_current_user_profile(
    db: Data<Db>,
    current_user: ReqData<User>,
    modify_user: Json<ModifyUser>,
) -> Result<Json<UserProfile>, DefaultApiError> {
    let modify_user = modify_user.into_inner();

    if let Err(e) = modify_user.validate() {
        log::warn!("API modify user validation error {}", e);
        return Err(DefaultApiError::ValidationFailed);
    }

    let db_user = crate::block(move || -> Result<User, DefaultApiError> {
        let modify_user = UpdateUser {
            title: modify_user.title,
            theme: modify_user.theme,
            language: modify_user.language,
            id_token_exp: None,
        };

        let modified_user = db.update_user(current_user.id, modify_user, None)?;

        Ok(modified_user.user)
    })
    .await??;

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
/// Returns the [`UserProfile`] of the requesting user.
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
/// Returns [`UserDetails`] of the specified user
#[get("/users/{user_id}")]
pub async fn user_details(
    db: Data<Db>,
    user_id: Path<UserId>,
) -> Result<Json<UserDetails>, DefaultApiError> {
    let user = crate::block(move || db.get_user_by_id(user_id.into_inner())).await??;

    let user_details = UserDetails {
        id: user.id,
        email: user.email,
        title: user.title,
        firstname: user.firstname,
        lastname: user.lastname,
    };

    Ok(Json(user_details))
}

#[derive(Deserialize)]
pub struct FindQuery {
    q: String,
}

/// API Endpoint *GET /users/find?name=$input*
///
/// Returns a list with a limited size of users matching the query
#[get("/users/find")]
pub async fn find(
    db: Data<Db>,
    query: Query<FindQuery>,
) -> Result<Json<Vec<UserDetails>>, DefaultApiError> {
    let found_users = crate::block(move || -> Result<Vec<User>, DefaultApiError> {
        Ok(db.find_users_by_name(&query.q)?)
    })
    .await??;

    let found_users = found_users.into_iter().map(Into::into).collect();

    Ok(Json(found_users))
}
