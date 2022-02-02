//! Auth related API structs and Endpoints
use super::{DefaultApiError, INVALID_ID_TOKEN};
use crate::ha_sync::user_update;
use crate::oidc::OidcContext;
use actix_web::web::{Data, Json};
use actix_web::{get, post};
use database::Db;
use db_storage::groups::GroupId;
use db_storage::users::{ModifiedUser, NewUser, NewUserWithGroups, UpdateUser, User};
use db_storage::DbUsersEx;
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
    db: Data<Db>,
    oidc_ctx: Data<OidcContext>,
    rabbitmq_channel: Data<lapin::Channel>,
    body: Json<Login>,
    authz: Data<kustos::Authz>,
) -> Result<Json<LoginResponse>, DefaultApiError> {
    let id_token = body.into_inner().id_token;

    match oidc_ctx.verify_id_token(&id_token) {
        Err(e) => {
            log::warn!("Got invalid ID Token {}", e);
            Err(DefaultApiError::auth_bearer_invalid_token(
                INVALID_ID_TOKEN,
                e.to_string(),
            ))
        }
        Ok(info) => {
            // TODO(r.floren): Find a neater way for relaying the information here.

            let db_result = crate::block(
                move || -> Result<Either<ModifiedUser, (User, Vec<GroupId>)>, DefaultApiError> {
                    let user = db.get_user_by_oidc_sub(&info.issuer, &info.sub)?;

                    match user {
                        Some(user) => {
                            let modify_user = UpdateUser {
                                title: None,
                                theme: None,
                                language: None,
                                id_token_exp: Some(info.expiration.timestamp()),
                            };

                            let modified_user =
                                db.update_user(user.id, modify_user, Some(info.x_grp))?;

                            Ok(Either::Left(modified_user))
                        }
                        None => {
                            let new_user = NewUser {
                                oidc_sub: info.sub,
                                oidc_issuer: info.issuer,
                                email: info.email,
                                title: String::new(),
                                firstname: info.firstname,
                                lastname: info.lastname,
                                id_token_exp: info.expiration.timestamp(),
                                theme: "default".to_string(),
                                language: "en-US".to_string(), // TODO: set language based on browser
                            };

                            let new_user = NewUserWithGroups {
                                new_user,
                                groups: info.x_grp.clone(),
                            };

                            let created = db.create_user(new_user)?;

                            Ok(Either::Right(created))
                        }
                    }
                },
            )
            .await??;

            if let Either::Left(modified_user) = &db_result {
                // The user was updated.
                let message = user_update::Message {
                    groups: modified_user.groups_changed,
                };
                if let Err(e) = message
                    .send_via(&*rabbitmq_channel, modified_user.user.id)
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
    db_result: Either<ModifiedUser, (User, Vec<GroupId>)>,
) -> Result<(), DefaultApiError> {
    match db_result {
        Either::Left(modified_user) => {
            // TODO(r.floren) this could be optimized I guess, with a user_to_groups?
            // But this is currently not a hot path.
            for group in modified_user.groups_added {
                authz
                    .add_user_to_group(modified_user.user.id, group)
                    .await
                    .map_err(|e| {
                        log::error!("Failed to add user to group - {}", e);
                        DefaultApiError::Internal
                    })?;
            }

            for group in modified_user.groups_removed {
                authz
                    .remove_user_from_group(modified_user.user.id, group)
                    .await
                    .map_err(|e| {
                        log::error!("Failed to remove user from group  - {}", e);
                        DefaultApiError::Internal
                    })?;
            }
        }
        Either::Right((user, groups)) => {
            authz.add_user_to_role(user.id, "user").await.map_err(|e| {
                log::error!("Failed to add user to user role - {}", e);
                DefaultApiError::Internal
            })?;

            for group in groups {
                authz.add_user_to_group(user.id, group).await.map_err(|e| {
                    log::error!("Failed to remove user from group  - {}", e);
                    DefaultApiError::Internal
                })?;
            }
        }
    }
    Ok(())
}
