//! Contains invite related REST endpoints.
use crate::api::v1::ApiResponse;
use crate::api::v1::{
    users::PublicUserProfile, DefaultApiError, DefaultApiResult, PagePaginationQuery,
};
use crate::settings::SharedSettingsActix;
use actix_web::web::{Data, Json, Path, Query, ReqData};
use actix_web::{delete, get, post, put, HttpResponse};
use chrono::{DateTime, Utc};
use database::{DatabaseError, Db};
use db_storage::invites::DbInvitesEx;
use db_storage::invites::{self as db_invites, InviteCodeId};
use db_storage::rooms::{DbRoomsEx, RoomId};
use db_storage::users::User;
use kustos::policies_builder::PoliciesBuilder;
use kustos::prelude::*;
use serde::{Deserialize, Serialize};
use validator::Validate;

/// Public invite details
///
/// Contains general public information about a room.
#[derive(Debug, Serialize)]
pub struct Invite {
    pub invite_code: InviteCodeId,
    pub created: DateTime<Utc>,
    pub created_by: PublicUserProfile,
    pub updated: DateTime<Utc>,
    pub updated_by: PublicUserProfile,
    pub room_id: RoomId,
    pub active: bool,
    pub expiration: Option<DateTime<Utc>>,
}

impl Invite {
    fn from_with_user(
        val: db_invites::Invite,
        created_by: PublicUserProfile,
        updated_by: PublicUserProfile,
    ) -> Self {
        Invite {
            invite_code: val.id,
            created: val.created_at,
            created_by,
            updated: val.updated_at,
            updated_by,
            room_id: val.room,
            active: val.active,
            expiration: val.expiration,
        }
    }
}

/// Body for *POST /rooms/{room_id}/invites*
#[derive(Debug, Deserialize)]
pub struct NewInvite {
    pub expiration: Option<DateTime<Utc>>,
}

/// Body for *PUT /rooms/{room_id}/invites/{invite_code}*
#[derive(Debug, Deserialize)]
pub struct UpdateInvite {
    pub expiration: Option<DateTime<Utc>>,
}

/// API Endpoint *POST /rooms/{room_id}/invites*
///
/// Uses the provided [`NewInvite`] to create a new invite.
#[post("/rooms/{room_id}/invites")]
pub async fn add_invite(
    settings: SharedSettingsActix,
    db: Data<Db>,
    authz: Data<Authz>,
    current_user: ReqData<User>,
    room_id: Path<RoomId>,
    data: Json<NewInvite>,
) -> DefaultApiResult<Invite> {
    let settings = settings.load_full();
    let room_id = room_id.into_inner();
    let current_user = current_user.into_inner();

    let new_invite = data.into_inner();
    let db_clone = db.clone();
    let current_user_clone = current_user.clone();
    let (db_invite, created_by, updated_by) = crate::block(
        move || -> Result<db_invites::InviteWithUsers, DefaultApiError> {
            let room = db_clone
                .get_room(room_id)?
                .ok_or(DefaultApiError::NotFound)?;

            if room.created_by != current_user_clone.id {
                // Avoid leaking rooms the user has no access to.
                return Err(DefaultApiError::NotFound);
            }

            let new_invite = db_invites::NewInvite {
                active: true,
                created_by: current_user_clone.id,
                updated_by: current_user_clone.id,
                room: room.id,
                expiration: new_invite.expiration,
            };

            db.new_invite_with_users(new_invite).map_err(Into::into)
        },
    )
    .await??;

    // TODO(r.floren) Do we want to rollback if this failed?
    let rel_invite_path = format!("/rooms/{}/invites/{}", room_id, db_invite.id);
    let invite_path = db_invite.id.resource_id();

    let policies = PoliciesBuilder::new()
        .grant_user_access(current_user.id)
        .add_resource(
            rel_invite_path,
            [AccessMethod::Get, AccessMethod::Put, AccessMethod::Delete],
        )
        .add_resource(
            invite_path,
            [AccessMethod::Get, AccessMethod::Put, AccessMethod::Delete],
        )
        .finish();

    if let Err(e) = authz.add_policies(policies).await {
        log::error!("Failed to add RBAC policies: {}", e);
        return Err(DefaultApiError::Internal);
    }

    let created_by = PublicUserProfile::from_db(&settings, created_by);
    let updated_by = PublicUserProfile::from_db(&settings, updated_by);

    let invite = Invite::from_with_user(db_invite, created_by, updated_by);

    Ok(ApiResponse::new(invite))
}

/// API Endpoint *GET /rooms/{room_id}/invites*
///
/// Returns a JSON array of all accessible invites for the given room
#[get("/rooms/{room_id}/invites")]
pub async fn get_invites(
    settings: SharedSettingsActix,
    db: Data<Db>,
    authz: Data<Authz>,
    current_user: ReqData<User>,
    room_id: Path<RoomId>,
    pagination: Query<PagePaginationQuery>,
) -> DefaultApiResult<Vec<Invite>> {
    let settings = settings.load_full();
    let room_id = room_id.into_inner();
    let current_user = current_user.into_inner();
    let PagePaginationQuery { per_page, page } = pagination.into_inner();

    let accessible_rooms: kustos::AccessibleResources<InviteCodeId> = authz
        .get_accessible_resources_for_user(current_user.id, AccessMethod::Get)
        .await
        .map_err(|_| DefaultApiError::Internal)?;

    let (invites, total_invites) = crate::block(
        move || -> Result<(Vec<db_invites::InviteWithUsers>, i64), DefaultApiError> {
            let room = db.get_room(room_id)?;
            if let Some(room) = room {
                match accessible_rooms {
                    kustos::AccessibleResources::All => {
                        Ok(db.get_invites_for_room_with_users_paginated(room.id, per_page, page)?)
                    }
                    kustos::AccessibleResources::List(list) => Ok(db
                        .get_invites_for_room_with_users_by_ids_paginated(
                            room.id, &list, per_page, page,
                        )?),
                }
            } else {
                Err(DefaultApiError::NotFound)
            }
        },
    )
    .await??;

    let invites = invites
        .into_iter()
        .map(|(db_invite, created_by, updated_by)| {
            let created_by = PublicUserProfile::from_db(&settings, created_by);
            let updated_by = PublicUserProfile::from_db(&settings, updated_by);

            Invite::from_with_user(db_invite, created_by, updated_by)
        })
        .collect::<Vec<Invite>>();

    Ok(ApiResponse::new(invites).with_page_pagination(per_page, page, total_invites))
}

#[derive(Debug, Deserialize)]
pub struct RoomIdAndInviteCode {
    room_id: RoomId,
    invite_code: InviteCodeId,
}

/// API Endpoint *GET /rooms/{room_id}/invites/{invite_code}*
///
/// Returns a single invite.
/// Returns 401 Not Found when the user has no access.
#[get("/rooms/{room_id}/invites/{invite_code}")]
pub async fn get_invite(
    settings: SharedSettingsActix,
    db: Data<Db>,
    current_user: ReqData<User>,
    path_params: Path<RoomIdAndInviteCode>,
) -> DefaultApiResult<Invite> {
    let settings = settings.load_full();

    let RoomIdAndInviteCode {
        room_id,
        invite_code,
    } = path_params.into_inner();

    let (db_invite, created_by, updated_by) = crate::block(
        move || -> Result<(db_invites::Invite, User, User), DefaultApiError> {
            let room = db.get_room(room_id)?;
            if room.is_some() {
                let result = db.get_invite_with_users(invite_code)?;
                if !check_owning_access(&result.0, &current_user.into_inner()) {
                    return Err(DefaultApiError::NotFound);
                }
                Ok(result)
            } else {
                Err(DefaultApiError::NotFound)
            }
        },
    )
    .await??;

    let created_by = PublicUserProfile::from_db(&settings, created_by);
    let updated_by = PublicUserProfile::from_db(&settings, updated_by);

    Ok(ApiResponse::new(Invite::from_with_user(
        db_invite, created_by, updated_by,
    )))
}

/// API Endpoint *PUT /rooms/{room_id}/invites/{invite_code}*
///
/// Uses the provided [`UpdateInvite`] to modify a specified invite.
/// Returns the modified [`Invite`]
#[put("/rooms/{room_id}/invites/{invite_code}")]
pub async fn update_invite(
    settings: SharedSettingsActix,
    db: Data<Db>,
    current_user: ReqData<User>,
    path_params: Path<RoomIdAndInviteCode>,
    update_invite: Json<UpdateInvite>,
) -> DefaultApiResult<Invite> {
    let settings = settings.load_full();
    let RoomIdAndInviteCode {
        room_id,
        invite_code,
    } = path_params.into_inner();
    let update_invite = update_invite.into_inner();

    let (db_invite, created_by, updated_by) = crate::block(
        move || -> Result<(db_invites::Invite, User, User), DefaultApiError> {
            let room = db.get_room(room_id)?;
            if room.is_some() {
                Ok(db.get_invite(invite_code).and_then(|invite| {
                    let current_user = current_user.into_inner();
                    if check_owning_access(&invite, &current_user) {
                        let now = Utc::now();
                        let update_invite = db_invites::UpdateInvite {
                            updated_by: Some(current_user.id),
                            updated_at: Some(now),
                            expiration: Some(update_invite.expiration),
                            active: None,
                            room: None,
                        };

                        db.update_invite_with_users(invite_code, &update_invite)
                    } else {
                        Err(DatabaseError::NotFound)
                    }
                })?)
            } else {
                Err(DefaultApiError::NotFound)
            }
        },
    )
    .await??;

    let created_by = PublicUserProfile::from_db(&settings, created_by);
    let updated_by = PublicUserProfile::from_db(&settings, updated_by);

    Ok(ApiResponse::new(Invite::from_with_user(
        db_invite, created_by, updated_by,
    )))
}

/// API Endpoint *PUT /rooms/{room_id}*
///
/// Deletes the [`Invite`] identified by this resource.
/// Returns 204 No Content
#[delete("/rooms/{room_id}/invites/{invite_code}")]
pub async fn delete_invite(
    db: Data<Db>,
    authz: Data<kustos::Authz>,
    current_user: ReqData<User>,
    path_params: Path<RoomIdAndInviteCode>,
) -> Result<HttpResponse, DefaultApiError> {
    let RoomIdAndInviteCode {
        room_id,
        invite_code,
    } = path_params.into_inner();

    crate::block(move || -> Result<db_invites::Invite, DefaultApiError> {
        let room = db.get_room(room_id)?;
        if room.is_some() {
            let now = Utc::now();
            let invite_update = db_invites::UpdateInvite {
                updated_by: Some(current_user.id),
                updated_at: Some(now),
                expiration: None,
                active: Some(false),
                room: None,
            };
            Ok(db.get_invite(invite_code).and_then(|invite| {
                if check_owning_access(&invite, &current_user) {
                    // Make sure to mimic a not found when trying to delete a inactive invite, for now.
                    if !invite.active {
                        return Err(DatabaseError::NotFound);
                    }
                    db.update_invite(invite_code, &invite_update)
                } else {
                    Err(DatabaseError::NotFound)
                }
            })?)
        } else {
            Err(DefaultApiError::NotFound)
        }
    })
    .await??;

    let rel_invite_path = format!("/rooms/{}/invites/{}", room_id, invite_code);
    let invite_path = invite_code.resource_id();

    if let Err(e) = authz
        .remove_explicit_resource_permissions(rel_invite_path)
        .await
    {
        log::error!("failed to remove resource {}", e);
    }

    if let Err(e) = authz
        .remove_explicit_resource_permissions(invite_path)
        .await
    {
        log::error!("failed to remove resource {}", e);
    }

    Ok(HttpResponse::NoContent().finish())
}

#[derive(Debug, Validate, Deserialize)]
pub struct VerifyBody {
    invite_code: InviteCodeId,
}

#[derive(Debug, Serialize)]
pub struct CodeVerified {
    room_id: RoomId,
}

/// API Endpoint *POST /invite/verify*
///
/// Used to verify a invite_code via POST.
/// As the GET request might not be Idempotent this should be the prioritized endpoint to verify invite_codes.
#[post("/invite/verify")]
pub async fn verify_invite_code(
    db: Data<Db>,
    data: Json<VerifyBody>,
) -> DefaultApiResult<CodeVerified> {
    let data = data.into_inner();

    if let Err(e) = data.validate() {
        log::warn!("API new room validation error {}", e);
        return Err(DefaultApiError::ValidationFailed);
    }

    let db_clone = db.clone();
    let invite = crate::block(move || -> Result<db_invites::Invite, DefaultApiError> {
        Ok(db_clone.get_invite(data.invite_code)?)
    })
    .await??;

    if invite.active {
        if let Some(expiration) = invite.expiration {
            if expiration <= Utc::now() {
                // Do not leak the existence of the invite when it is expired
                return Err(DefaultApiError::NotFound);
            }
        }
        Ok(ApiResponse::new(CodeVerified {
            room_id: invite.room,
        }))
    } else {
        // TODO(r.floren) Do we want to return something else here?
        Err(DefaultApiError::NotFound)
    }
}

fn check_owning_access(invite: &db_invites::Invite, user: &User) -> bool {
    invite.created_by == user.id || invite.updated_by == user.id
}
