//! Auth related API structs and Endpoints
use super::events::EventPoliciesBuilderExt;
use super::rooms::RoomsPoliciesBuilderExt;
use crate::api::v1::response::error::AuthenticationError;
use crate::api::v1::response::ApiError;
use crate::ha_sync::user_update;
use crate::oidc::OidcContext;
use crate::settings::SharedSettingsActix;
use actix_web::web::{Data, Json};
use actix_web::{get, post};
use controller_shared::settings::CallIn;
use database::{Db, OptionalExt};
use db_storage::events::email_invites::EventEmailInvite;
use db_storage::events::EventId;
use db_storage::groups::GroupId;
use db_storage::rooms::RoomId;
use db_storage::users::{NewUser, NewUserWithGroups, UpdateUser, User, UserUpdatedInfo};
use kustos::prelude::PoliciesBuilder;
use lapin_pool::RabbitMqChannel;
use phonenumber::Mode;
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

                        Ok(LoginResult::UserUpdated(updated_info))
                    }
                    None => {
                        let settings = settings.load_full();

                        let phone_number = if let Some((call_in, phone_number)) =
                            settings.call_in.as_ref().zip(info.phone_number)
                        {
                            parse_phone_number(call_in, phone_number)
                        } else {
                            None
                        };

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

/// Parses the phone number and formats it as E.164
fn parse_phone_number(call_in_settings: &CallIn, phone_number: String) -> Option<String> {
    match phonenumber::parse(Some(call_in_settings.default_country_code), phone_number) {
        Ok(phone) => {
            if phone.is_valid() {
                let phone = phone.format().mode(Mode::E164).to_string();

                Some(phone)
            } else {
                log::warn!("A phone number provided by the oidc provider is invalid");
                None
            }
        }
        Err(err) => {
            log::error!(
                "Failed to parse a phone number provided by the oidc provider {}",
                err
            );
            None
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
