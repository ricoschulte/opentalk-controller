//! Auth related API structs and Endpoints
use super::events::EventPoliciesBuilderExt;
use super::rooms::RoomsPoliciesBuilderExt;
use crate::api::util::parse_phone_number;
use crate::api::v1::response::error::AuthenticationError;
use crate::api::v1::response::ApiError;
use crate::ha_sync::user_update;
use crate::oidc::{IdTokenInfo, OidcContext, VerifyError};
use crate::settings::SharedSettingsActix;
use actix_web::web::{Data, Json};
use actix_web::{get, post};
use controller_shared::settings::Settings;
use database::Db;
use db_storage::events::email_invites::EventEmailInvite;
use db_storage::events::EventId;
use db_storage::groups::GroupId;
use db_storage::rooms::RoomId;
use db_storage::users::{NewUser, NewUserWithGroups, UpdateUser, User, UserUpdatedInfo};
use kustos::prelude::PoliciesBuilder;
use lapin_pool::RabbitMqChannel;
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
    rabbitmq_channel: Data<RabbitMqChannel>,
    body: Json<Login>,
    authz: Data<kustos::Authz>,
) -> Result<Json<LoginResponse>, ApiError> {
    let id_token = body.into_inner().id_token;

    match oidc_ctx.verify_id_token(&id_token) {
        Err(e) => {
            log::warn!("Got invalid ID Token {}", e);
            match e {
                VerifyError::InvalidClaims => Err(ApiError::bad_request()
                    .with_code("invalid_claims")
                    .with_message("some required attributes are missing or malformed")),
                VerifyError::Expired(_) => Err(ApiError::unauthorized()
                    .with_www_authenticate(AuthenticationError::SessionExpired)),
                VerifyError::MissingKeyID
                | VerifyError::UnknownKeyID
                | VerifyError::InvalidJwt(_)
                | VerifyError::InvalidSignature => Err(ApiError::unauthorized()
                    .with_www_authenticate(AuthenticationError::InvalidIdToken)),
            }
        }
        Ok(info) => {
            // TODO(r.floren): Find a neater way for relaying the information here.

            let db_result = crate::block(move || -> database::Result<_> {
                let conn = db.get_conn()?;

                let user = User::get_by_oidc_sub(&conn, &info.issuer, &info.sub)?;

                let settings = settings.load_full();

                match user {
                    Some(user) => {
                        let changeset = create_changeset(&settings, &user, &info);

                        let updated_info = changeset.apply(&conn, user.id, Some(&info.x_grp))?;

                        Ok(LoginResult::UserUpdated(updated_info))
                    }
                    None => {
                        let phone_number = if let Some((call_in, phone_number)) =
                            settings.call_in.as_ref().zip(info.phone_number)
                        {
                            parse_phone_number(&phone_number, call_in.default_country_code)
                                .map(|p| p.format().mode(phonenumber::Mode::E164).to_string())
                        } else {
                            None
                        };

                        let display_name = info
                            .display_name
                            .unwrap_or_else(|| format!("{} {}", info.firstname, info.lastname));

                        let new_user = NewUser {
                            oidc_sub: info.sub,
                            oidc_issuer: info.issuer,
                            email: info.email,
                            title: String::new(),
                            display_name,
                            firstname: info.firstname,
                            lastname: info.lastname,
                            id_token_exp: info.expiration.timestamp(),
                            // TODO: try to get user language from accept-language header
                            language: settings.defaults.user_language.clone(),
                            phone: phone_number,
                        };

                        let new_user = NewUserWithGroups {
                            new_user,
                            groups: info.x_grp,
                        };

                        let (user, group_ids) = new_user.insert(&conn)?;

                        let event_and_room_ids =
                            EventEmailInvite::migrate_to_user_invites(&conn, user.id, &user.email)?;

                        Ok(LoginResult::UserCreated {
                            user,
                            group_ids,
                            event_and_room_ids,
                        })
                    }
                }
            })
            .await??;

            if let LoginResult::UserUpdated(updated_info) = &db_result {
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

/// Create an [`UpdateUser`] changeset based on a comparison between `user` and `token_info`
fn create_changeset<'a>(
    settings: &Settings,
    user: &User,
    token_info: &'a IdTokenInfo,
) -> UpdateUser<'a> {
    let User {
        id: _,
        id_serial: _,
        oidc_sub: _,
        oidc_issuer: _,
        email,
        title: _,
        firstname,
        lastname,
        id_token_exp: _,
        language: _,
        display_name: _,
        dashboard_theme: _,
        conference_theme: _,
        phone,
    } = user;

    let mut changeset = UpdateUser {
        id_token_exp: Some(token_info.expiration.timestamp()),
        ..Default::default()
    };

    if firstname != &token_info.firstname {
        changeset.firstname = Some(&token_info.firstname);
    }

    if lastname != &token_info.lastname {
        changeset.lastname = Some(&token_info.lastname)
    }

    if email != &token_info.email {
        changeset.email = Some(&token_info.email);
    }

    let token_phone = if let Some((call_in, phone_number)) = settings
        .call_in
        .as_ref()
        .zip(token_info.phone_number.as_deref())
    {
        parse_phone_number(phone_number, call_in.default_country_code)
            .map(|p| p.format().mode(phonenumber::Mode::E164).to_string())
    } else {
        None
    };

    if phone != &token_phone {
        changeset.phone = Some(token_phone)
    }

    changeset
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

enum LoginResult {
    UserCreated {
        user: User,
        group_ids: Vec<GroupId>,
        event_and_room_ids: Vec<(EventId, RoomId)>,
    },
    UserUpdated(UserUpdatedInfo),
}

async fn update_core_user_permissions(
    authz: &kustos::Authz,
    db_result: LoginResult,
) -> Result<(), ApiError> {
    match db_result {
        LoginResult::UserUpdated(modified_user) => {
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
        LoginResult::UserCreated {
            user,
            group_ids,
            event_and_room_ids,
        } => {
            authz.add_user_to_role(user.id, "user").await?;

            for group in group_ids {
                authz.add_user_to_group(user.id, group).await?;
            }

            // Migrate email invites to user invites
            // Add permissions for user to events that the email was invited to
            if event_and_room_ids.is_empty() {
                return Ok(());
            }

            let mut policies = PoliciesBuilder::new().grant_user_access(user.id);

            for (event_id, room_id) in event_and_room_ids {
                policies = policies
                    .event_read_access(event_id)
                    .room_read_access(room_id)
                    .event_invite_invitee_access(event_id);
            }

            authz.add_policies(policies.finish()).await?;
        }
    }

    Ok(())
}
