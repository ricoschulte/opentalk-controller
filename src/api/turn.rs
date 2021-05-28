//! TURN related API structs and Endpoints
use super::ApiError;
use crate::api::verify_access_token;
use crate::db::DbInterface;
use crate::oidc::OidcContext;
use actix_web::web::Data;
use actix_web::{get, HttpRequest, HttpResponse};
use ring::{rand, hmac};
use serde::{Serialize};
use crate::settings;

/// TURN access credentials for users.
#[derive(Debug, Serialize)]
pub struct Turn {
    pub username: String,
    pub password: String,
    pub ttl: String,
    pub uris: Vec<String>,
}

/// API Endpoint *GET /turn*
///
/// Returns a list of ['Turn'] with HMAC-SHA1 credentials following https://datatracker.ietf.org/doc/html/draft-uberti-behave-turn-rest-00
#[get("/turn")]
pub async fn get(
    db_ctx: Data<DbInterface>,
    oidc_ctx: Data<OidcContext>,
    turn_servers: Data<Option<settings::Turn>>,
    request: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let current_user = verify_access_token(&request, db_ctx.clone(), &oidc_ctx).await?;
    log::trace!("Generating new turn credentials for user {} and servers {:?}", current_user.oidc_uuid, turn_servers);

    let turn_servers: &Option<settings::Turn> = turn_servers.as_ref();
    let turn_credentials = turn_servers.as_ref().ok_or(ApiError::Internal).map(|turn| {
        let expires = (chrono::Utc::now() + turn.lifetime ).timestamp();

        // Create a list of TURN responses for each configured TURN server.
        turn.servers.iter().map(|server| {
            let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, server.pre_shared_key.as_bytes());
            let rng = rand::SystemRandom::new();
            let random_part: String = base64::encode(rand::generate::<[u8; 32]>(&rng).expect("Failed to generate random").expose().as_ref());
            let username = format!("{}:turn_random_for_privacy_{}", expires, random_part);
            let password= base64::encode(hmac::sign(&key, username.as_bytes()).as_ref());
            
            Turn {
                username,
                password,
                ttl: expires.to_string(),
                uris: server.uris.clone(),
            }
        }).collect::<Vec<_>>()
    })?;

    Ok(HttpResponse::Ok().json(turn_credentials))
}
