use super::ApiError;
use crate::oidc::OidcContext;
use actix_web::post;
use actix_web::web::{Data, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The JSON Body expected when making a call on `/auth/login`
#[derive(Debug, Deserialize)]
pub struct Login {
    id_token: String,
}

/// JSON Body of the response coming from '/auth/login/ endpoint
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    /// Permissions is a set of strings that each define a permission a user has.
    permissions: HashSet<String>,
}

#[post("/auth/login")]
pub async fn login(
    body: Json<Login>,
    oidc_ctx: Data<OidcContext>,
) -> Result<Json<LoginResponse>, ApiError> {
    let id_token = body.into_inner().id_token;

    match oidc_ctx.verify_id_token(&id_token).await {
        Err(e) => {
            log::warn!("Got invalid ID Token {}", e);
            Err(ApiError::InvalidIdToken)
        }
        Ok(_info) => {
            // TODO use info to make DB entry

            Ok(Json(LoginResponse {
                // TODO calculate permissions
                permissions: Default::default(),
            }))
        }
    }
}
