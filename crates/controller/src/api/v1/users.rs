//! User related API structs and Endpoints
//!
//! The defined structs are exposed to the REST API and will be serialized/deserialized. Similar
//! structs are defined in the Database module [`crate::db`] for database operations.

use crate::api::v1::{ApiResponse, DefaultApiError, PagePaginationQuery};
use crate::settings::SharedSettingsActix;
use actix_web::web::{Data, Json, Path, Query, ReqData};
use actix_web::{get, patch};
use controller_shared::settings::Settings;
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
pub struct PublicUserProfile {
    pub id: UserId,
    pub email: String,
    pub title: String,
    pub firstname: String,
    pub lastname: String,
    pub display_name: String,
    pub avatar_url: String,
}

impl PublicUserProfile {
    pub fn from_db(settings: &Settings, user: User) -> Self {
        let avatar_url = format!(
            "{}{:x}",
            settings.avatar.libravatar_url,
            md5::compute(&user.email)
        );

        Self {
            id: user.id,
            email: user.email,
            title: user.title,
            firstname: user.firstname,
            lastname: user.lastname,
            display_name: user.display_name,
            avatar_url,
        }
    }
}

/// Private user profile.
///
/// Similar to [`UserDetails`], but contains additional "private" information about a user.
/// Is only accessible to the user himself.
/// Is used on */users/me* endpoints.
#[derive(Debug, Serialize)]
pub struct PrivateUserProfile {
    pub id: UserId,
    pub email: String,
    pub title: String,
    pub firstname: String,
    pub lastname: String,
    pub display_name: String,
    pub avatar_url: String,
    pub dashboard_theme: String,
    pub conference_theme: String,
    pub language: String,
}

impl PrivateUserProfile {
    pub fn from_db(settings: &Settings, user: User) -> Self {
        let avatar_url = format!(
            "{}{:x}",
            settings.avatar.libravatar_url,
            md5::compute(&user.email)
        );

        Self {
            id: user.id,
            email: user.email,
            title: user.title,
            firstname: user.firstname,
            lastname: user.lastname,
            display_name: user.display_name,
            dashboard_theme: user.dashboard_theme,
            conference_theme: user.conference_theme,
            avatar_url,
            language: user.language,
        }
    }
}

/// API Endpoint *GET /users*
///
/// Returns a JSON array of all database users as [`UserDetails`]
#[get("/users")]
pub async fn all(
    settings: SharedSettingsActix,
    db: Data<Db>,
    current_user: ReqData<User>,
    pagination: Query<PagePaginationQuery>,
    authz: Data<Authz>,
) -> Result<ApiResponse<Vec<PublicUserProfile>>, DefaultApiError> {
    let settings = settings.load_full();
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
        .map(|db_user| PublicUserProfile::from_db(&settings, db_user))
        .collect::<Vec<PublicUserProfile>>();

    Ok(ApiResponse::new(users).with_page_pagination(per_page, page, total_users))
}

// Used to modify user settings
#[derive(Debug, Validate, Deserialize)]
#[validate(schema(function = "disallow_empty"))]
pub struct PatchMeBody {
    #[validate(length(max = 255))]
    pub title: Option<String>,
    #[validate(length(max = 255))]
    pub display_name: Option<String>,
    #[validate(length(max = 35))]
    pub language: Option<String>,
    #[validate(length(max = 128))]
    pub dashboard_theme: Option<String>,
    #[validate(length(max = 128))]
    pub conference_theme: Option<String>,
}

fn disallow_empty(patch: &PatchMeBody) -> Result<(), ValidationError> {
    let PatchMeBody {
        title,
        display_name,
        language,
        dashboard_theme,
        conference_theme,
    } = patch;

    if title.is_none()
        && display_name.is_none()
        && language.is_none()
        && dashboard_theme.is_none()
        && conference_theme.is_none()
    {
        Err(ValidationError::new(
            "patch body must have at least one valid field",
        ))
    } else {
        Ok(())
    }
}

/// API Endpoint *PATCH /users/me*
#[patch("/users/me")]
pub async fn patch_me(
    settings: SharedSettingsActix,
    db: Data<Db>,
    current_user: ReqData<User>,
    patch: Json<PatchMeBody>,
) -> Result<Json<PrivateUserProfile>, DefaultApiError> {
    let settings = settings.load_full();
    let patch = patch.into_inner();

    if let Err(e) = patch.validate() {
        log::warn!("API patch/me validation error {}", e);
        return Err(DefaultApiError::ValidationFailed);
    }

    let db_user = crate::block(move || -> Result<User, DefaultApiError> {
        let modify_user = UpdateUser {
            title: patch.title,
            display_name: patch.display_name,
            language: patch.language,
            dashboard_theme: patch.dashboard_theme,
            conference_theme: patch.conference_theme,
            id_token_exp: None,
        };

        let modified_user = db.update_user(current_user.id, modify_user, None)?;

        Ok(modified_user.user)
    })
    .await??;

    let user_profile = PrivateUserProfile::from_db(&settings, db_user);

    Ok(Json(user_profile))
}

/// API Endpoint *GET /users/me*
///
/// Returns the [`PrivateUserProfile`] of the requesting user.
#[get("/users/me")]
pub async fn get_me(
    settings: SharedSettingsActix,
    current_user: ReqData<User>,
) -> Result<Json<PrivateUserProfile>, DefaultApiError> {
    let settings = settings.load_full();
    let current_user = current_user.into_inner();

    let user_profile = PrivateUserProfile::from_db(&settings, current_user);

    Ok(Json(user_profile))
}

/// API Endpoint *GET /users/{user_id}*
///
/// Returns [`PubUserProfile`] of the specified user
#[get("/users/{user_id}")]
pub async fn get_user(
    settings: SharedSettingsActix,
    db: Data<Db>,
    user_id: Path<UserId>,
) -> Result<Json<PublicUserProfile>, DefaultApiError> {
    let settings = settings.load_full();

    let user = crate::block(move || db.get_user_by_id(user_id.into_inner())).await??;

    let user_profile = PublicUserProfile::from_db(&settings, user);

    Ok(Json(user_profile))
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
    settings: SharedSettingsActix,
    db: Data<Db>,
    query: Query<FindQuery>,
) -> Result<Json<Vec<PublicUserProfile>>, DefaultApiError> {
    let settings = settings.load_full();

    let found_users = crate::block(move || -> Result<Vec<User>, DefaultApiError> {
        Ok(db.find_users_by_name(&query.q)?)
    })
    .await??;

    let found_users = found_users
        .into_iter()
        .map(|user| PublicUserProfile::from_db(&settings, user))
        .collect();

    Ok(Json(found_users))
}
