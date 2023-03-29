// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! Auth related API structs and Endpoints
use super::events::EventPoliciesBuilderExt;
use super::rooms::RoomsPoliciesBuilderExt;
use crate::api::v1::response::error::AuthenticationError;
use crate::api::v1::response::ApiError;
use crate::oidc::{OidcContext, VerifyError};
use crate::settings::SharedSettingsActix;
use actix_web::web::{Data, Json};
use actix_web::{get, post};
use controller_shared::settings::{TariffAssignment, TenantAssignment};
use core::mem::take;
use database::{Db, OptionalExt};
use db_storage::groups::{get_or_create_groups_by_name, Group};
use db_storage::tariffs::{ExternalTariffId, Tariff};
use db_storage::tenants::{get_or_create_tenant_by_oidc_id, OidcTenantId, TenantId};
use db_storage::users::User;
use kustos::prelude::PoliciesBuilder;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use types::core::{EventId, GroupName, RoomId};

mod create_user;
mod update_user;

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
    body: Json<Login>,
    authz: Data<kustos::Authz>,
) -> Result<Json<LoginResponse>, ApiError> {
    let id_token = body.into_inner().id_token;

    let mut info = match oidc_ctx.verify_id_token(&id_token) {
        Ok(info) => info,
        Err(e) => {
            return match e {
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
            };
        }
    };

    let db_result = crate::block(move || -> Result<_, ApiError> {
        let settings = settings.load_full();
        let mut conn = db.get_conn()?;

        // Get tariff depending on the configured assignment
        let tariff = match &settings.tariffs.assignment {
            TariffAssignment::Static { static_tariff_name } => {
                Tariff::get_by_name(&mut conn, static_tariff_name)?
            }
            TariffAssignment::ByExternalTariffId => {
                let external_tariff_id = info.tariff_id.clone().ok_or_else(|| {
                    ApiError::bad_request()
                        .with_code("invalid_claims")
                        .with_message("tariff_id missing in id_token claims")
                })?;

                Tariff::get_by_external_id(&mut conn, &ExternalTariffId::from(external_tariff_id))
                    .optional()?
                    .ok_or_else(|| {
                        ApiError::internal()
                            .with_code("invalid_tariff_id")
                            .with_message("JWT contained unknown tariff_id")
                    })?
            }
        };

        // Get the tenant_id depending on the configured assignment
        let tenant_id = match &settings.tenants.assignment {
            TenantAssignment::Static { static_tenant_id } => static_tenant_id.clone(),
            TenantAssignment::ByExternalTenantId => info.tenant_id.clone().ok_or_else(|| {
                ApiError::bad_request()
                    .with_code("invalid_claims")
                    .with_message("tenant_id missing in id_token claims")
            })?,
        };

        let tenant = get_or_create_tenant_by_oidc_id(&mut conn, &OidcTenantId::from(tenant_id))?;

        let groups: Vec<(TenantId, GroupName)> = take(&mut info.x_grp)
            .into_iter()
            .map(|group| (tenant.id, GroupName::from(group)))
            .collect();

        let groups = get_or_create_groups_by_name(&mut conn, &groups)?;

        // Try to get the user by the `sub` field in the IdToken
        let user = User::get_by_oidc_sub(&mut conn, tenant.id, &info.sub)?;

        let login_result = match user {
            Some(user) => {
                // Found a matching user, update its attributes, tenancy and groups
                update_user::update_user(&settings, &mut conn, user, info, groups, tariff)?
            }
            None => {
                // No matching user, create a new one with inside the given tenants and groups
                create_user::create_user(&settings, &mut conn, info, tenant, groups, tariff)?
            }
        };

        Ok(login_result)
    })
    .await??;

    update_core_user_permissions(authz.as_ref(), db_result).await?;

    Ok(Json(LoginResponse {
        // TODO calculate permissions
        permissions: Default::default(),
    }))
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
        groups: Vec<Group>,
        event_and_room_ids: Vec<(EventId, RoomId)>,
    },
    UserUpdated {
        user: User,
        groups_added_to: Vec<Group>,
        groups_removed_from: Vec<Group>,
    },
}

async fn update_core_user_permissions(
    authz: &kustos::Authz,
    db_result: LoginResult,
) -> Result<(), ApiError> {
    match db_result {
        LoginResult::UserUpdated {
            user,
            groups_added_to,
            groups_removed_from,
        } => {
            // TODO(r.floren) this could be optimized I guess, with a user_to_groups?
            // But this is currently not a hot path.
            for group in groups_added_to {
                authz.add_user_to_group(user.id, group.id).await?;
            }

            for group in groups_removed_from {
                authz.remove_user_from_group(user.id, group.id).await?;
            }
        }
        LoginResult::UserCreated {
            user,
            groups,
            event_and_room_ids,
        } => {
            authz.add_user_to_role(user.id, "user").await?;

            for group in groups {
                authz.add_user_to_group(user.id, group.id).await?;
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
