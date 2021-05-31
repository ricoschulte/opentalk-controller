//! TURN related API structs and Endpoints
use super::ApiError;
use crate::db::users::User;
use crate::settings;
use actix_web::get;
use actix_web::web::Data;
use actix_web::web::Json;
use actix_web::web::ReqData;
use ring::{hmac, rand};
use serde::Serialize;

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
    turn_servers: Data<Option<settings::Turn>>,
    current_user: ReqData<User>,
) -> Result<Json<Vec<Turn>>, ApiError> {
    log::trace!(
        "Generating new turn credentials for user {} and servers {:?}",
        current_user.oidc_uuid,
        turn_servers
    );

    let turn_servers: &Option<settings::Turn> = turn_servers.as_ref();
    let turn_credentials = turn_servers
        .as_ref()
        .ok_or(ApiError::Internal)
        .map(|turn| {
            let expires = (chrono::Utc::now() + turn.lifetime).timestamp();

            // Create a list of TURN responses for each configured TURN server.
            // Todo we should invest time to add SHA265 support to coturn or our own turn server.
            turn.servers
                .iter()
                .map(|server| {
                    let key = hmac::Key::new(
                        hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
                        server.pre_shared_key.as_bytes(),
                    );
                    let rng = rand::SystemRandom::new();

                    // We append 16 bytes as a base64 encoded string to the prefix `turn_random_for_privacy_` for usage as application data in our username
                    let random_part: String = base64::encode(
                        rand::generate::<[u8; 16]>(&rng)
                            .expect("Failed to generate random")
                            .expose()
                            .as_ref(),
                    );
                    let username = format!("{}:turn_random_for_privacy_{}", expires, random_part);
                    let password = base64::encode(hmac::sign(&key, username.as_bytes()).as_ref());

                    Turn {
                        username,
                        password,
                        ttl: expires.to_string(),
                        uris: server.uris.clone(),
                    }
                })
                .collect::<Vec<_>>()
        })?;

    Ok(Json(turn_credentials))
}
