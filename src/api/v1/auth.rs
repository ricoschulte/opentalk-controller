//! Auth related API structs and Endpoints
use super::{DefaultApiError, INVALID_ID_TOKEN};
use crate::db;
use crate::db::users::ModifiedUser;
use crate::db::DbInterface;
use crate::ha_sync::user_update;
use crate::oidc::OidcContext;
use actix_web::web::{Data, Json};
use actix_web::{get, post, web};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::str::FromStr;

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
    db_ctx: Data<DbInterface>,
    oidc_ctx: Data<OidcContext>,
    rabbitmq_channel: Data<lapin::Channel>,
    body: Json<Login>,
) -> Result<Json<LoginResponse>, DefaultApiError> {
    let id_token = body.into_inner().id_token;

    match oidc_ctx.verify_id_token(&id_token).await {
        Err(e) => {
            log::warn!("Got invalid ID Token {}", e);
            Err(DefaultApiError::auth_bearer_invalid_token(
                INVALID_ID_TOKEN,
                e.to_string(),
            ))
        }
        Ok(info) => {
            let user_uuid = match uuid::Uuid::from_str(&info.sub) {
                Ok(uuid) => uuid,
                Err(_) => {
                    log::error!("Unable to parse UUID from id token sub '{}'", &info.sub);
                    return Err(DefaultApiError::auth_bearer_invalid_token(
                        INVALID_ID_TOKEN,
                        "Unable to parse UUID from id token".into(),
                    ));
                }
            };

            let modified_user =
                web::block(move || -> Result<Option<ModifiedUser>, DefaultApiError> {
                    let user = db_ctx.get_user_by_uuid(&user_uuid)?;

                    match user {
                        Some(user) => {
                            let modify_user = db::users::ModifyUser {
                                title: None,
                                theme: None,
                                language: None,
                                id_token_exp: Some(info.expiration.timestamp()),
                            };

                            let modified_user = db_ctx.modify_user(
                                user.oidc_uuid,
                                modify_user,
                                Some(info.x_grp),
                            )?;

                            Ok(Some(modified_user))
                        }
                        None => {
                            let new_user = db::users::NewUser {
                                oidc_uuid: user_uuid,
                                email: info.email,
                                title: String::new(),
                                firstname: info.firstname,
                                lastname: info.lastname,
                                id_token_exp: info.expiration.timestamp(),
                                theme: "default".to_string(),
                                language: "en-US".to_string(), // TODO: set language based on browser
                            };

                            let new_user = db::users::NewUserWithGroups {
                                new_user,
                                groups: info.x_grp,
                            };

                            db_ctx.create_user(new_user)?;

                            Ok(None)
                        }
                    }
                })
                .await
                .map_err(|e| {
                    log::error!("BlockingError on POST /auth/login - {}", e);
                    DefaultApiError::Internal
                })??;

            if let Some(modified_user) = modified_user {
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
