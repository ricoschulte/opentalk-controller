// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! TURN related API structs and Endpoints
use super::response::ApiError;
use crate::api::v1::middleware::user_auth::check_access_token;
use crate::api::v1::response::error::AuthenticationError;
use crate::api::v1::response::NoContent;
use crate::oidc::OidcContext;
use crate::settings::{self, SharedSettingsActix};
use crate::settings::{Settings, TurnServer};
use actix_http::StatusCode;
use actix_web::http::header::Header;
use actix_web::web::Data;
use actix_web::web::Json;
use actix_web::Either as AWEither;
use actix_web::HttpRequest;
use actix_web::{get, ResponseError};
use actix_web_httpauth::headers::authorization::{Authorization, Bearer};
use arc_swap::ArcSwap;
use database::{Db, OptionalExt};
use db_storage::invites::{Invite, InviteCodeId};
use db_storage::users::User;
use either::Either;
use openidconnect::AccessToken;
use rand::distributions::{Distribution, Uniform};
use rand::prelude::SliceRandom;
use rand::{CryptoRng, Rng};
use ring::hmac;
use serde::Serialize;
use std::str::FromStr;

/// TURN access credentials for users.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Turn {
    pub username: String,
    pub password: String,
    pub ttl: String,
    pub uris: Vec<String>,
}

/// STUN Server for users.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Stun {
    pub uris: Vec<String>,
}

impl From<&settings::Stun> for Stun {
    fn from(stun: &settings::Stun) -> Self {
        Stun {
            uris: stun.uris.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum IceServer {
    Turn(Turn),
    Stun(Stun),
}

/// API Endpoint *GET /turn*
///
/// Returns a list of ['Turn'] with HMAC-SHA1 credentials following <https://datatracker.ietf.org/doc/html/draft-uberti-behave-turn-rest-00>
#[get("/turn")]
pub async fn get(
    settings: SharedSettingsActix,
    db: Data<Db>,
    oidc_ctx: Data<OidcContext>,
    req: HttpRequest,
) -> Result<AWEither<Json<Vec<IceServer>>, NoContent>, ApiError> {
    let settings: &ArcSwap<Settings> = &settings;
    let settings = settings.load();

    let turn_servers = settings.turn.clone();
    let stun_servers = &settings.stun;

    // This is a omniauth endpoint. AccessTokens and InviteCodes are allowed as Bearer tokens
    match check_access_token_or_invite(&settings, &req, db, oidc_ctx).await? {
        Either::Right(invite) => {
            log::trace!(
                "Generating new turn credentials for invite {} and servers {:?}",
                invite.id,
                &turn_servers
            );
        }
        Either::Left(user) => {
            log::trace!(
                "Generating new turn credentials for user {} and servers {:?}",
                user.id,
                &turn_servers
            );
        }
    }
    let mut ice_servers = match turn_servers {
        Some(turn_config) => {
            let expires = (chrono::Utc::now()
                + chrono::Duration::from_std(turn_config.lifetime)
                    .map_err(|_| ApiError::internal())?)
            .timestamp();
            let mut rand_rng = ::rand::thread_rng();
            rr_servers(&mut rand_rng, &turn_config.servers, expires)
        }
        None => vec![],
    };

    if let Some(stun_config) = stun_servers {
        ice_servers.push(IceServer::Stun(Stun {
            uris: stun_config.uris.clone(),
        }));
    }

    if ice_servers.is_empty() {
        return Ok(AWEither::Right(NoContent {}));
    }

    Ok(AWEither::Left(Json(ice_servers)))
}

fn create_credentials<T: Rng + CryptoRng>(
    rng: &mut T,
    psk: &str,
    ttl: i64,
    uris: &[String],
) -> IceServer {
    // TODO We should invest time to add SHA265 support to coturn or our own turn server.
    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, psk.as_bytes());

    // We append 16 bytes as a base64 encoded string to the prefix `turn_random_for_privacy_` for usage as application data in our username
    let random_part: String = base64::encode(rng.gen::<[u8; 16]>().as_ref());
    let username = format!("{ttl}:turn_random_for_privacy_{random_part}",);
    let password = base64::encode(hmac::sign(&key, username.as_bytes()).as_ref());

    IceServer::Turn(Turn {
        username,
        password,
        ttl: ttl.to_string(),
        uris: uris.to_vec(),
    })
}

fn rr_servers<T: Rng + CryptoRng>(
    rng: &mut T,
    servers: &[TurnServer],
    expires: i64,
) -> Vec<IceServer> {
    // Create a list of TURN responses for each configured TURN server.
    match servers.len() {
        0 => vec![],
        // When we only have one configured TURN server, return the credentials for this single one.
        1 => vec![create_credentials(rng, &servers[0].pre_shared_key, expires, &servers[0].uris)]
        ,
        // When we have two configured TURN servers, draw a random one and return the credentials for this drawn one.
        2 => {
            let between: Uniform<u32> = Uniform::from(0..1);
            let selected_server = between.sample(rng) as usize;
            let turn = create_credentials(
                rng,
                &servers[selected_server].pre_shared_key,
                expires,
                &servers[selected_server].uris,
            );

            vec![turn]
        }
        // When we have more than two configured TURN servers, draw two and return the credentials for the drawn ones.
        _ => servers
            .choose_multiple(rng, 2)
            .map(|server| {
                create_credentials(rng, &server.pre_shared_key, expires, &server.uris)
            })
            .collect::<Vec<_>>(),
    }
}

/// Checks for a valid access_token similar to the OIDC Middleware, but also allows invite_tokens as a valid bearer token.
async fn check_access_token_or_invite(
    settings: &Settings,
    req: &HttpRequest,
    db: Data<Db>,
    oidc_ctx: Data<OidcContext>,
) -> Result<Either<User, Invite>, ApiError> {
    let auth = Authorization::<Bearer>::parse(req).map_err(|e| {
        log::warn!("Unable to parse access token, {}", e);
        ApiError::unauthorized()
            .with_message("Unable to parse Authentication Bearer header")
            .with_www_authenticate(AuthenticationError::InvalidAccessToken)
    })?;

    let access_token = auth.into_scheme().token().to_string();
    let current_user = check_access_token(
        settings,
        db.clone(),
        oidc_ctx,
        AccessToken::new(access_token.clone()),
    )
    .await;

    match current_user {
        Ok((_, user)) => Ok(Either::Left(user)),
        Err(e) => {
            // return early if we got a non-auth error
            if e.status_code() != StatusCode::UNAUTHORIZED {
                return Err(e);
            }

            let invite_uuid = uuid::Uuid::from_str(&access_token).map_err(|_| {
                ApiError::unauthorized()
                    .with_www_authenticate(AuthenticationError::InvalidAccessToken)
            })?;

            crate::block(move || {
                let mut conn = db.get_conn()?;

                match Invite::get(&mut conn, InviteCodeId::from(invite_uuid)).optional()? {
                    Some(invite) => Ok(invite),
                    None => {
                        log::warn!("The requesting user could not be found in the database");
                        Err(ApiError::unauthorized()
                            .with_www_authenticate(AuthenticationError::InvalidAccessToken))
                    }
                }
            })
            .await?
            .map(Either::Right)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_create_credentials() {
        use rand::prelude::*;
        use rand::SeedableRng;
        let mut rng = StdRng::seed_from_u64(1234567890);
        let credentials =
            create_credentials(&mut rng, "PSK", 3400, &["turn:turn.turn.turn".to_owned()]);
        assert_eq!(
            credentials,
            IceServer::Turn(Turn {
                username: "3400:turn_random_for_privacy_8VbonSpZc9GXSw9gMxaV0A==".to_owned(),
                password: "h3R6Ob2G0+nH3oRhO2y/IuK757Y=".to_owned(),
                ttl: 3400.to_string(),
                uris: vec!["turn:turn.turn.turn".to_owned()]
            })
        );
    }

    #[test]
    fn test_round_robin() {
        use rand::prelude::*;
        use rand::SeedableRng;

        // No configured servers
        let mut rng = StdRng::seed_from_u64(1234567890);
        assert_eq!(rr_servers(&mut rng, &[], 1200), vec![]);

        // One configured server
        let mut rng = StdRng::seed_from_u64(1234567890);
        let one_server = vec![TurnServer {
            uris: vec!["turn:turn1.turn.turn".to_owned()],
            pre_shared_key: "PSK1".to_owned(),
        }];
        assert_eq!(
            rr_servers(&mut rng, &one_server, 1200),
            vec![IceServer::Turn(Turn {
                username: "1200:turn_random_for_privacy_8VbonSpZc9GXSw9gMxaV0A==".to_owned(),
                password: "G7hjqVZX/dVAOgzzb+GeS8vEpcU=".to_owned(),
                ttl: 1200.to_string(),
                uris: vec!["turn:turn1.turn.turn".to_owned()]
            })]
        );

        // Two configured servers
        let mut rng = StdRng::seed_from_u64(1234567890);
        let two_servers = vec![
            TurnServer {
                uris: vec!["turn:turn1.turn.turn".to_owned()],
                pre_shared_key: "PSK1".to_owned(),
            },
            TurnServer {
                uris: vec!["turn:turn2.turn.turn".to_owned()],
                pre_shared_key: "PSK2".to_owned(),
            },
        ];
        assert_eq!(
            rr_servers(&mut rng, &two_servers, 1200),
            vec![IceServer::Turn(Turn {
                username: "1200:turn_random_for_privacy_VuidKllz0ZdLD2AzFpXQYA==".to_owned(),
                password: "Aybo95/GPrJhWN2qqbz6yP2qEvg=".to_owned(),
                ttl: 1200.to_string(),
                uris: vec!["turn:turn1.turn.turn".to_owned()]
            })]
        );

        // Three configured servers
        let mut rng = StdRng::seed_from_u64(1234567890);
        let three_servers = vec![
            TurnServer {
                uris: vec!["turn:turn1.turn.turn".to_owned()],
                pre_shared_key: "PSK1".to_owned(),
            },
            TurnServer {
                uris: vec!["turn:turn2.turn.turn".to_owned()],
                pre_shared_key: "PSK2".to_owned(),
            },
            TurnServer {
                uris: vec!["turn:turn3.turn.turn".to_owned()],
                pre_shared_key: "PSK3".to_owned(),
            },
        ];
        assert_eq!(
            rr_servers(&mut rng, &three_servers, 1200),
            vec![
                IceServer::Turn(Turn {
                    username: "1200:turn_random_for_privacy_nSpZc9GXSw9gMxaV0GDahQ==".to_owned(),
                    password: "6zfptEfCPlF3oWFtPKtlAPwjAWs=".to_owned(),
                    ttl: 1200.to_string(),
                    uris: vec!["turn:turn1.turn.turn".to_owned()]
                }),
                IceServer::Turn(Turn {
                    username: "1200:turn_random_for_privacy_AYzBIYeOCHhhiwR7CQ3X1A==".to_owned(),
                    password: "fiWX+emwV1thN/dBcZ9melA061g=".to_owned(),
                    ttl: 1200.to_string(),
                    uris: vec!["turn:turn3.turn.turn".to_owned()]
                })
            ]
        );

        // Test uniformity
        let mut first = 0;
        let mut second = 0;
        let mut third = 0;
        for _ in 1..5000 {
            rr_servers(&mut rng, &three_servers, 1200)
                .iter()
                .filter_map(|e| match e {
                    IceServer::Turn(turn) => Some(turn),
                    _ => None,
                })
                .for_each(|e| match e.uris[0].as_ref() {
                    "turn:turn1.turn.turn" => first += 1,
                    "turn:turn2.turn.turn" => second += 1,
                    "turn:turn3.turn.turn" => third += 1,
                    _ => unreachable!(),
                });
        }

        // Makeshift test if the samples are uniform, instead of something like a chi-squared test
        assert!(first as f64 > second as f64 * 0.95);
        assert!(first as f64 > third as f64 * 0.95);

        assert!(second as f64 > third as f64 * 0.95);
        assert!(second as f64 > first as f64 * 0.95);

        assert!(third as f64 > first as f64 * 0.95);
        assert!(third as f64 > second as f64 * 0.95);
    }
}
