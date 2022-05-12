//! Auth related API structs and Endpoints
use crate::api::v1::response::error::AuthenticationError;
use crate::api::v1::response::ApiError;
use crate::ha_sync::user_update;
use crate::oidc::OidcContext;
use crate::settings::SharedSettingsActix;
use actix_web::web::{Data, Json};
use actix_web::{get, post};
use database::{Db, OptionalExt};
use db_storage::groups::GroupId;
use db_storage::users::{NewUser, NewUserWithGroups, UpdateUser, User, UserUpdatedInfo};
use either::Either;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The JSON Body expected when making a *POST* request on `/auth/login`
#[derive(Debug, Deserialize)]
pub struct Login {
    id_token: String,
}

/// JSON Body of the response coming from the *POST* request on `/auth/login/`
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    /// Permissions is a set of strings that each define a permission a user has.
    permissions: HashSet<String>,
}

/// API Endpoint *POST /auth/login*
///
/// Verifies the `id_token` inside the provided [`Json<Login>`] body. When the token is valid, a
/// database lookup for the requesting user is issued, if no user is found, a new user will be created.
///
/// Returns a [`LoginResponse`] containing the users permissions.
#[post("/auth/login")]
pub async fn login(
    settings: SharedSettingsActix,
    db: Data<Db>,
    oidc_ctx: Data<OidcContext>,
    rabbitmq_channel: Data<lapin::Channel>,
    body: Json<Login>,
    authz: Data<kustos::Authz>,
) -> Result<Json<LoginResponse>, ApiError> {
    let id_token = body.into_inner().id_token;

    match oidc_ctx.verify_id_token(&id_token) {
        Err(e) => {
            log::warn!("Got invalid ID Token {}", e);

            Err(ApiError::unauthorized().with_www_authenticate(AuthenticationError::InvalidIdToken))
        }
        Ok(info) => {
            // TODO(r.floren): Find a neater way for relaying the information here.

            let db_result = crate::block(move || -> database::Result<_> {
                let conn = db.get_conn()?;

                let user = User::get_by_oidc_sub(&conn, &info.issuer, &info.sub).optional()?;

                match user {
                    Some(user) => {
                        let changeset = UpdateUser {
                            id_token_exp: Some(info.expiration.timestamp()),
                            ..Default::default()
                        };

                        let updated_info = changeset.apply(&conn, user.id, Some(info.x_grp))?;

                        Ok(Either::Left(updated_info))
                    }
                    None => {
                        let settings = settings.load_full();

                        let new_user = NewUser {
                            oidc_sub: info.sub,
                            oidc_issuer: info.issuer,
                            email: info.email,
                            title: String::new(),
                            display_name: format!("{} {}", info.firstname, info.lastname),
                            firstname: info.firstname,
                            lastname: info.lastname,
                            id_token_exp: info.expiration.timestamp(),
                            // TODO: try to get user language from accept-language header
                            language: settings.defaults.user_language.clone(),
                        };

                        let new_user = NewUserWithGroups {
                            new_user,
                            groups: info.x_grp,
                        };

                        let user_with_groups = new_user.insert(&conn)?;

                        Ok(Either::Right(user_with_groups))
                    }
                }
            })
            .await??;

            if let Either::Left(updated_info) = &db_result {
                // The user was updated.
                let message = user_update::Message {
                    groups: updated_info.groups_changed,
                };

                if let Err(e) = message
                    .send_via(&*rabbitmq_channel, updated_info.user.id)
                    .await
                {
                    log::error!("Failed to send user-update message {:?}", e);
                }
            }

            update_core_user_permissions(authz.as_ref(), db_result).await?;

            Ok(Json(LoginResponse {
                // TODO calculate permissions
                permissions: Default::default(),
            }))
        }
    }
}

/// Wrapper struct for the oidc provider
#[derive(Debug, Serialize, Eq, PartialEq, Hash)]
pub struct Provider {
    oidc: OidcProvider,
}

/// Represents an OIDC provider
#[derive(Debug, Serialize, Eq, PartialEq, Hash)]
pub struct OidcProvider {
    name: String,
    url: String,
}

/// API Endpoint *GET /auth/login*
///
/// Returns information about the OIDC provider
#[get("/auth/login")]
pub async fn oidc_provider(oidc_ctx: Data<OidcContext>) -> Json<Provider> {
    let provider = OidcProvider {
        name: "default".to_string(),
        url: oidc_ctx.provider_url(),
    };

    Json(Provider { oidc: provider })
}

async fn update_core_user_permissions(
    authz: &kustos::Authz,
    db_result: Either<UserUpdatedInfo, (User, Vec<GroupId>)>,
) -> Result<(), ApiError> {
    match db_result {
        Either::Left(modified_user) => {
            // TODO(r.floren) this could be optimized I guess, with a user_to_groups?
            // But this is currently not a hot path.
            for group in modified_user.groups_added {
                authz
                    .add_user_to_group(modified_user.user.id, group)
                    .await?;
            }

            for group in modified_user.groups_removed {
                authz
                    .remove_user_from_group(modified_user.user.id, group)
                    .await?;
            }
        }
        Either::Right((user, groups)) => {
            authz.add_user_to_role(user.id, "user").await?;

            for group in groups {
                authz.add_user_to_group(user.id, group).await?;
            }
        }
    }
    Ok(())
}
