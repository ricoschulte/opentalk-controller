//! Auth related API structs and Endpoints
use super::{ApiError, INVALID_ID_TOKEN};
use crate::db;
use crate::db::DbInterface;
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
    body: Json<Login>,
) -> Result<Json<LoginResponse>, ApiError> {
    let id_token = body.into_inner().id_token;

    match oidc_ctx.verify_id_token(&id_token).await {
        Err(e) => {
            log::warn!("Got invalid ID Token {}", e);
            Err(ApiError::Auth(INVALID_ID_TOKEN, e.to_string()))
        }
        Ok(info) => {
            let user_uuid = match uuid::Uuid::from_str(&info.sub) {
                Ok(uuid) => uuid,
                Err(_) => {
                    log::error!("Unable to parse UUID from id token sub '{}'", &info.sub);
                    return Err(ApiError::Auth(
                        INVALID_ID_TOKEN,
                        "Unable to parse UUID from id token".to_string(),
                    ));
                }
            };

            web::block(move || -> Result<(), ApiError> {
                let user = db_ctx.get_user_by_uuid(&user_uuid)?;

                match user {
                    Some(user) => {
                        let modify_user = db::users::ModifyUser {
                            title: None,
                            theme: None,
                            language: None,
                            id_token_exp: Some(info.expiration.timestamp()),
                        };

                        db_ctx.modify_user(user.oidc_uuid, modify_user, Some(info.x_grp))?;

                        Ok(())
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

                        Ok(())
                    }
                }
            })
            .await
            .map_err(|e| {
                log::error!("BlockingError on POST /auth/login - {}", e);
                ApiError::Internal
            })??;

            Ok(Json(LoginResponse {
                // TODO calculate permissions
                permissions: Default::default(),
            }))
        }
    }
}

/// Represents an OIDC Provider
#[derive(Debug, Serialize, Eq, PartialEq, Hash)]
pub struct OidcProvider {
    name: String,
    url: String,
}

/// A set of OIDC Providers
///
/// JSON Body of the response for *GET '/auth/login/*
#[derive(Debug, Serialize)]
pub struct OidcProviders {
    providers: HashSet<OidcProvider>,
}

/// API Endpoint *GET /auth/login*
///
/// Returns a list of available OIDC providers
#[get("/auth/login")]
pub async fn oidc_providers(oidc_ctx: Data<OidcContext>) -> Result<Json<OidcProviders>, ApiError> {
    let mut providers = HashSet::new();

    providers.insert(OidcProvider {
        name: "default".to_string(),
        url: oidc_ctx.provider_url(),
    });

    Ok(Json(OidcProviders { providers }))
}
