//! TURN related API structs and Endpoints
use super::DefaultApiError;
use crate::api::v1::response::NoContent;
use crate::db::users::User;
use crate::settings;
use crate::settings::{Settings, TurnServer};
use actix_web::get;
use actix_web::web::Data;
use actix_web::web::Json;
use actix_web::web::ReqData;
use actix_web::Either;
use arc_swap::ArcSwap;
use rand::distributions::{Distribution, Uniform};
use rand::prelude::SliceRandom;
use rand::CryptoRng;
use rand::Rng;
use ring::hmac;
use serde::Serialize;

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
    settings: Data<arc_swap::ArcSwap<settings::Settings>>,
    current_user: ReqData<User>,
) -> Result<Either<Json<Vec<IceServer>>, NoContent>, DefaultApiError> {
    let settings: &ArcSwap<Settings> = &**settings;
    let settings = settings.load();

    let turn_servers = &settings.turn;
    let stun_servers = &settings.stun;

    log::trace!(
        "Generating new turn credentials for user {} and servers {:?}",
        current_user.oidc_uuid,
        turn_servers
    );

    let mut ice_servers = match turn_servers {
        Some(turn_config) => {
            let expires = (chrono::Utc::now() + turn_config.lifetime).timestamp();
            let mut rand_rng = ::rand::thread_rng();
            rr_servers(&mut rand_rng, &turn_config.servers, expires)?
        }
        None => vec![],
    };

    if let Some(stun_config) = stun_servers {
        ice_servers.push(IceServer::Stun(Stun {
            uris: stun_config.uris.clone(),
        }));
    }

    if ice_servers.is_empty() {
        return Ok(Either::Right(NoContent {}));
    }

    Ok(Either::Left(Json(ice_servers)))
}

fn create_credentials<T: Rng + CryptoRng>(
    rng: &mut T,
    psk: &str,
    ttl: i64,
    uris: &[String],
) -> Result<IceServer, anyhow::Error> {
    // TODO We should invest time to add SHA265 support to coturn or our own turn server.
    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, psk.as_bytes());

    // We append 16 bytes as a base64 encoded string to the prefix `turn_random_for_privacy_` for usage as application data in our username
    let random_part: String = base64::encode(rng.gen::<[u8; 16]>().as_ref());
    let username = format!("{}:turn_random_for_privacy_{}", ttl, random_part);
    let password = base64::encode(hmac::sign(&key, username.as_bytes()).as_ref());

    Ok(IceServer::Turn(Turn {
        username,
        password,
        ttl: ttl.to_string(),
        uris: uris.to_vec(),
    }))
}

fn rr_servers<T: Rng + CryptoRng>(
    rng: &mut T,
    servers: &[TurnServer],
    expires: i64,
) -> Result<Vec<IceServer>, DefaultApiError> {
    // Create a list of TURN responses for each configured TURN server.
    match servers.len() {
        0 => Ok(vec![]),
        // When we only have one configured TURN server, return the credentials for this single one.
        1 => match create_credentials(rng, &servers[0].pre_shared_key, expires, &servers[0].uris) {
            Ok(turn) => Ok(vec![turn]),
            Err(e) => {
                log::error!("TURN credential error: {}", e);
                Err(DefaultApiError::Internal)
            }
        },
        // When we have two configured TURN servers, draw a random one and return the credentials for this drawn one.
        2 => {
            let between: Uniform<u32> = Uniform::from(0..1);
            let selected_server = between.sample(rng) as usize;
            match create_credentials(
                rng,
                &servers[selected_server].pre_shared_key,
                expires,
                &servers[selected_server].uris,
            ) {
                Ok(turn) => Ok(vec![turn]),
                Err(e) => {
                    log::error!("TURN credential error: {}", e);
                    Err(DefaultApiError::Internal)
                }
            }
        }
        // When we have more than two configured TURN servers, draw two and return the credentials for the drawn ones.
        _ => servers
            .choose_multiple(rng, 2)
            .into_iter()
            .map(|server| {
                match create_credentials(rng, &server.pre_shared_key, expires, &server.uris) {
                    Ok(turn) => Ok(turn),
                    Err(e) => {
                        log::error!("TURN credential error: {}", e);
                        Err(DefaultApiError::Internal)
                    }
                }
            })
            .collect::<Result<Vec<_>, DefaultApiError>>(),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_create_credentials() {
        use rand::prelude::*;
        use rand::SeedableRng;
        let mut rng = StdRng::seed_from_u64(1234567890);
        let credentials =
            create_credentials(&mut rng, "PSK", 3400, &["turn:turn.turn.turn".to_owned()]).unwrap();
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
        assert_eq!(rr_servers(&mut rng, &[], 1200).unwrap(), vec![]);

        // One configured server
        let mut rng = StdRng::seed_from_u64(1234567890);
        let one_server = vec![TurnServer {
            uris: vec!["turn:turn1.turn.turn".to_owned()],
            pre_shared_key: "PSK1".to_owned(),
        }];
        assert_eq!(
            rr_servers(&mut rng, &one_server, 1200).unwrap(),
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
            rr_servers(&mut rng, &two_servers, 1200).unwrap(),
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
            rr_servers(&mut rng, &three_servers, 1200).unwrap(),
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
                .unwrap()
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
