//! User related API structs and Endpoints
//!
//! The defined structs are exposed to the REST API and will be serialized/deserialized. Similar
//! structs are defined in the Database crate [`db_storage`] for database operations.

use super::response::{ApiError, NoContent};
use crate::settings::SharedSettingsActix;
use actix_web::web::{Data, Json, Path, Query, ReqData};
use actix_web::{get, patch, Either};
use anyhow::Context;
use controller_shared::settings::Settings;
use database::Db;
use db_storage::users::{UpdateUser, User, UserId};
use keycloak_admin::KeycloakAdminClient;
use serde::{Deserialize, Serialize};
use validator::Validate;

/// Public user details.
///
/// Contains general "public" information about a user. Is accessible to all other users.
#[derive(Debug, Clone, Serialize)]
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
        let avatar_url = email_to_libravatar_url(&settings.avatar.libravatar_url, &user.email);

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

pub fn email_to_libravatar_url(libravatar_url: &str, email: &str) -> String {
    format!("{}{:x}", libravatar_url, md5::compute(email))
}

/// Private user profile.
///
/// Similar to [`PublicUserProfile`], but contains additional "private" information about a user.
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

// Used to modify user settings
#[derive(Debug, Validate, Deserialize)]
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

impl PatchMeBody {
    fn is_empty(&self) -> bool {
        let PatchMeBody {
            title,
            display_name,
            language,
            dashboard_theme,
            conference_theme,
        } = self;

        title.is_none()
            && display_name.is_none()
            && language.is_none()
            && dashboard_theme.is_none()
            && conference_theme.is_none()
    }
}

/// API Endpoint *PATCH /users/me*
#[patch("/users/me")]
pub async fn patch_me(
    settings: SharedSettingsActix,
    db: Data<Db>,
    current_user: ReqData<User>,
    patch: Json<PatchMeBody>,
) -> Result<Either<Json<PrivateUserProfile>, NoContent>, ApiError> {
    let patch = patch.into_inner();

    if patch.is_empty() {
        return Ok(Either::Right(NoContent));
    }

    patch.validate()?;

    let settings = settings.load_full();

    let db_user = crate::block(move || -> Result<User, ApiError> {
        let mut conn = db.get_conn()?;

        let changeset = UpdateUser {
            title: patch.title.as_deref(),
            firstname: None,
            lastname: None,
            phone: None,
            email: None,
            display_name: patch.display_name.as_deref(),
            language: patch.language.as_deref(),
            dashboard_theme: patch.dashboard_theme.as_deref(),
            conference_theme: patch.conference_theme.as_deref(),
            id_token_exp: None,
        };

        let updated_info = changeset.apply(&mut conn, current_user.id, None)?;

        Ok(updated_info.user)
    })
    .await??;

    let user_profile = PrivateUserProfile::from_db(&settings, db_user);

    Ok(Either::Left(Json(user_profile)))
}

/// API Endpoint *GET /users/me*
///
/// Returns the [`PrivateUserProfile`] of the requesting user.
#[get("/users/me")]
pub async fn get_me(
    settings: SharedSettingsActix,
    current_user: ReqData<User>,
) -> Result<Json<PrivateUserProfile>, ApiError> {
    let settings = settings.load_full();
    let current_user = current_user.into_inner();

    let user_profile = PrivateUserProfile::from_db(&settings, current_user);

    Ok(Json(user_profile))
}

/// API Endpoint *GET /users/{user_id}*
///
/// Returns [`PublicUserProfile`] of the specified user
#[get("/users/{user_id}")]
pub async fn get_user(
    settings: SharedSettingsActix,
    db: Data<Db>,
    user_id: Path<UserId>,
) -> Result<Json<PublicUserProfile>, ApiError> {
    let settings = settings.load_full();

    let user = crate::block(move || {
        let mut conn = db.get_conn()?;

        User::get(&mut conn, user_id.into_inner())
    })
    .await??;

    let user_profile = PublicUserProfile::from_db(&settings, user);

    Ok(Json(user_profile))
}

#[derive(Deserialize)]
pub struct FindQuery {
    q: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum UserFindResponseItem {
    Registered(PublicUserProfile),
    Unregistered(UnregisteredUser),
}

#[derive(Debug, Clone, Serialize)]
pub struct UnregisteredUser {
    pub email: String,
    pub firstname: String,
    pub lastname: String,
    pub avatar_url: String,
}

/// API Endpoint *GET /users/find?name=$input*
///
/// Returns a list with a limited size of users matching the query
#[get("/users/find")]
pub async fn find(
    settings: SharedSettingsActix,
    kc_admin_client: Data<KeycloakAdminClient>,
    current_user: ReqData<User>,
    db: Data<Db>,
    query: Query<FindQuery>,
) -> Result<Json<Vec<UserFindResponseItem>>, ApiError> {
    let settings = settings.load_full();

    if settings.endpoints.disable_users_find {
        return Err(ApiError::not_found());
    }

    let found_users = if settings.endpoints.users_find_use_kc {
        let mut found_kc_users = kc_admin_client
            .search_user(&query.q)
            .await
            .context("Failed to search for user in keycloak")?;

        let (db_users, kc_users) = crate::block(move || -> Result<_, ApiError> {
            let mut conn = db.get_conn()?;

            let kc_subs: Vec<&str> = found_kc_users
                .iter()
                .map(|kc_user| kc_user.id.as_str())
                .collect();

            let users = User::get_all_by_oidc_subs(&mut conn, &current_user.oidc_issuer, &kc_subs)?;

            found_kc_users.retain(|kc_user| !users.iter().any(|user| user.oidc_sub == kc_user.id));

            Ok((users, found_kc_users))
        })
        .await??;

        db_users
            .into_iter()
            .map(|user| {
                UserFindResponseItem::Registered(PublicUserProfile::from_db(&settings, user))
            })
            .chain(kc_users.into_iter().map(|kc_user| {
                let avatar_url =
                    email_to_libravatar_url(&settings.avatar.libravatar_url, &kc_user.email);

                UserFindResponseItem::Unregistered(UnregisteredUser {
                    email: kc_user.email,
                    firstname: kc_user.first_name,
                    lastname: kc_user.last_name,
                    avatar_url,
                })
            }))
            .collect()
    } else {
        let found_users = crate::block(move || {
            let mut conn = db.get_conn()?;

            User::find(&mut conn, &query.q)
        })
        .await??;

        found_users
            .into_iter()
            .map(|user| {
                UserFindResponseItem::Registered(PublicUserProfile::from_db(&settings, user))
            })
            .collect()
    };

    Ok(Json(found_users))
}
