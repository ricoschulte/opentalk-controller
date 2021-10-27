//! Contains invite related REST endpoints.
use crate::api::v1::ApiResponse;
use crate::api::v1::{users::UserDetails, DefaultApiError, DefaultApiResult, PagePaginationQuery};
use crate::db::invites::{self as db_invites, InviteCodeUuid};
use crate::db::rooms::RoomId;
use crate::db::users::{self as db_users, User};
use crate::db::{DatabaseError, DbInterface};
use actix_web::web::{self, Data, Json, Path, ReqData};
use actix_web::{delete, get, post, put, HttpResponse};
use serde::{Deserialize, Serialize};
use validator::Validate;

/// Public invite details
///
/// Contains general public information about a room.
#[derive(Debug, Serialize)]
pub struct Invite {
    pub invite_code: String,
    pub created: chrono::DateTime<chrono::Utc>,
    pub created_by: UserDetails,
    pub updated: chrono::DateTime<chrono::Utc>,
    pub updated_by: UserDetails,
    pub room_id: RoomId,
    pub active: bool,
    pub expiration: Option<chrono::DateTime<chrono::Utc>>,
}
impl Invite {
    fn from_with_user<U>(val: db_invites::Invite, created_by: U, updated_by: U) -> Self
    where
        U: Into<UserDetails>,
    {
        Invite {
            invite_code: val.uuid.to_string(),
            created: val.created,
            created_by: created_by.into(),
            updated: val.updated,
            updated_by: updated_by.into(),
            room_id: val.room,
            active: val.active,
            expiration: val.expiration,
        }
    }
}

/// Body for *POST /rooms/{room_uuid}/invites*
#[derive(Debug, Deserialize)]
pub struct NewInvite {
    pub expiration: Option<chrono::DateTime<chrono::Utc>>,
}

/// Body for *PUT /rooms/{room_uuid}/invites/{invite_code}*
#[derive(Debug, Deserialize)]
pub struct UpdateInvite {
    pub expiration: Option<chrono::DateTime<chrono::Utc>>,
}

/// API Endpoint *POST /rooms/{room_uuid}/invites*
///
/// Uses the provided [`NewInvite`] to create a new invite.
#[post("/rooms/{room_uuid}/invites")]
pub async fn add_invite(
    db_ctx: Data<DbInterface>,
    current_user: ReqData<User>,
    room_uuid: Path<RoomId>,
    data: Json<NewInvite>,
) -> DefaultApiResult<Invite> {
    let room_uuid = room_uuid.into_inner();

    let new_invite = data.into_inner();
    let db_ctx_clone = db_ctx.clone();
    let (db_invite, created_by, updated_by) = crate::block(
        move || -> Result<db_invites::InviteWithUsers, DefaultApiError> {
            let room = db_ctx_clone
                .get_room(room_uuid)?
                .ok_or(DefaultApiError::NotFound)?;

            if room.owner != current_user.id {
                // Avoid leaking rooms the user has no access to.
                return Err(DefaultApiError::NotFound);
            }

            let invite_code_uuid = uuid::Uuid::new_v4();
            let now = chrono::Utc::now();
            let new_invite = db_invites::NewInvite {
                uuid: &InviteCodeUuid::from(invite_code_uuid),
                active: true,
                created: &now,
                created_by: &current_user.id,
                updated: &now,
                updated_by: &current_user.id,
                room: &room.uuid,
                expiration: new_invite.expiration.as_ref(),
            };

            db_ctx.new_invite_with_users(new_invite).map_err(Into::into)
        },
    )
    .await
    .map_err(|e| {
        log::error!("BlockingError on GET /rooms/{}/invites - {}", room_uuid, e);
        DefaultApiError::Internal
    })??;

    let output = ApiResponse::new(Invite::from_with_user(db_invite, created_by, updated_by));
    Ok(output)
}

/// API Endpoint *GET /rooms/{room_uuid}/invites*
///
/// Returns a JSON array of all accessible invites for the given room
#[get("/rooms/{room_uuid}/invites")]
pub async fn get_invites(
    db_ctx: Data<DbInterface>,
    room_uuid: Path<RoomId>,
    current_user: ReqData<User>,
    pagination: web::Query<PagePaginationQuery>,
) -> DefaultApiResult<Vec<Invite>> {
    let room_uuid = room_uuid.into_inner();
    let PagePaginationQuery { per_page, page } = pagination.into_inner();

    let (invites, total_invites) = crate::block(
        move || -> Result<(Vec<db_invites::InviteWithUsers>, i64), DefaultApiError> {
            let room = db_ctx.get_room(room_uuid)?;
            if let Some(room) = room {
                Ok(db_ctx.get_invites_for_user_paginated_with_users(
                    &room,
                    &current_user.into_inner(),
                    per_page as i64,
                    page as i64,
                )?)
            } else {
                Err(DefaultApiError::NotFound)
            }
        },
    )
    .await
    .map_err(|e| {
        log::error!("BlockingError on GET /rooms/{}/invites - {}", room_uuid, e);
        DefaultApiError::Internal
    })??;

    let invites = invites
        .into_iter()
        .map(|(db_invite, created_by, updated_by)| {
            Invite::from_with_user(db_invite, created_by, updated_by)
        })
        .collect::<Vec<Invite>>();

    Ok(ApiResponse::new(invites).with_page_pagination(per_page as i64, page as i64, total_invites))
}

#[derive(Debug, Deserialize)]
pub struct RoomIdAndInviteCode {
    room_uuid: RoomId,
    invite_code: InviteCodeUuid,
}

/// API Endpoint *GET /rooms/{room_uuid}/invites/{invite_code}*
///
/// Returns a single invite.
/// Returns 401 Not Found when the user has no access.
#[get("/rooms/{room_uuid}/invites/{invite_code}")]
pub async fn get_invite(
    db_ctx: Data<DbInterface>,
    current_user: ReqData<User>,
    path_params: Path<RoomIdAndInviteCode>,
) -> DefaultApiResult<Invite> {
    let RoomIdAndInviteCode {
        room_uuid,
        invite_code,
    } = path_params.into_inner();

    let invite_code_clone = invite_code.clone();
    let (db_invite, created_by, updated_by) = crate::block(
        move || -> Result<(db_invites::Invite, db_users::User, db_users::User), DefaultApiError> {
            let room = db_ctx.get_room(room_uuid)?;
            if room.is_some() {
                let result = db_ctx.get_invite_with_users(&invite_code_clone)?;
                if !check_owning_access(&result.0, &current_user.into_inner()) {
                    return Err(DefaultApiError::NotFound);
                }
                Ok(result)
            } else {
                Err(DefaultApiError::NotFound)
            }
        },
    )
    .await
    .map_err(|e| {
        log::error!(
            "BlockingError on GET /rooms/{}/invites/{} - {}",
            room_uuid,
            invite_code,
            e
        );
        DefaultApiError::Internal
    })??;

    Ok(ApiResponse::new(Invite::from_with_user(
        db_invite, created_by, updated_by,
    )))
}

/// API Endpoint *PUT /rooms/{room_uuid}/invites/{invite_code}*
///
/// Uses the provided [`UpdateInvite`] to modify a specified invite.
/// Returns the modified [`Invite`]
#[put("/rooms/{room_uuid}/invites/{invite_code}")]
pub async fn update_invite(
    db_ctx: Data<DbInterface>,
    current_user: ReqData<User>,
    path_params: Path<RoomIdAndInviteCode>,
    update_invite: Json<UpdateInvite>,
) -> DefaultApiResult<Invite> {
    let RoomIdAndInviteCode {
        room_uuid,
        invite_code,
    } = path_params.into_inner();
    let update_invite = update_invite.into_inner();

    let invite_code_clone = invite_code.clone();
    let (db_invite, created_by, updated_by) = crate::block(
        move || -> Result<(db_invites::Invite, db_users::User, db_users::User), DefaultApiError> {
            let room = db_ctx.get_room(room_uuid)?;
            if room.is_some() {
                Ok(db_ctx.get_invite(&invite_code_clone).and_then(|invite| {
                    let current_user = current_user.into_inner();
                    if check_owning_access(&invite, &current_user) {
                        let now = chrono::Utc::now();
                        let update_invite = db_invites::UpdateInvite {
                            updated_by: Some(&current_user.id),
                            updated: Some(&now),
                            expiration: Some(update_invite.expiration.as_ref()),
                            active: None,
                            room: None,
                        };

                        db_ctx.update_invite_with_users(&invite_code_clone, &update_invite)
                    } else {
                        Err(DatabaseError::NotFound)
                    }
                })?)
            } else {
                Err(DefaultApiError::NotFound)
            }
        },
    )
    .await
    .map_err(|e| {
        log::error!(
            "BlockingError on PUT /rooms/{}/invites/{} - {}",
            room_uuid,
            invite_code,
            e
        );
        DefaultApiError::Internal
    })??;

    Ok(ApiResponse::new(Invite::from_with_user(
        db_invite, created_by, updated_by,
    )))
}

/// API Endpoint *PUT /rooms/{room_uuid}*
///
/// Deletes the [`Invite`] identified by this resource.
/// Returns 204 No Content
#[delete("/rooms/{room_uuid}/invites/{invite_code}")]
pub async fn delete_invite(
    db_ctx: Data<DbInterface>,
    current_user: ReqData<User>,
    path_params: Path<RoomIdAndInviteCode>,
) -> Result<HttpResponse, DefaultApiError> {
    let RoomIdAndInviteCode {
        room_uuid,
        invite_code,
    } = path_params.into_inner();
    let invite_code_clone = invite_code.clone();

    crate::block(move || -> Result<db_invites::Invite, DefaultApiError> {
        let room = db_ctx.get_room(room_uuid)?;
        if room.is_some() {
            let now = chrono::Utc::now();
            let invite_update = db_invites::UpdateInvite {
                updated_by: Some(&current_user.id),
                updated: Some(&now),
                expiration: None,
                active: Some(false),
                room: None,
            };
            Ok(db_ctx.get_invite(&invite_code_clone).and_then(|invite| {
                if check_owning_access(&invite, &current_user) {
                    // Make sure to mimic a not found when trying to delete a inactive invite, for now.
                    if !invite.active {
                        return Err(DatabaseError::NotFound);
                    }
                    db_ctx.update_invite(&invite_code_clone, &invite_update)
                } else {
                    Err(DatabaseError::NotFound)
                }
            })?)
        } else {
            Err(DefaultApiError::NotFound)
        }
    })
    .await
    .map_err(|e| {
        log::error!(
            "BlockingError on DELETE /rooms/{}/invites/{} - {}",
            room_uuid,
            invite_code,
            e
        );
        DefaultApiError::Internal
    })??;

    Ok(HttpResponse::NoContent().finish())
}

#[derive(Debug, Validate, Deserialize)]
pub struct VerifyBody {
    invite_code: uuid::Uuid,
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
    db_ctx: Data<DbInterface>,
    data: Json<VerifyBody>,
) -> DefaultApiResult<CodeVerified> {
    let data = data.into_inner();

    if let Err(e) = data.validate() {
        log::warn!("API new room validation error {}", e);
        return Err(DefaultApiError::ValidationFailed);
    }

    let invite_code = data.invite_code;
    let db_ctx_clone = db_ctx.clone();
    let invite = crate::block(move || -> Result<db_invites::Invite, DefaultApiError> {
        Ok(db_ctx_clone.get_invite(&InviteCodeUuid::from(invite_code))?)
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on GET /invite/{} - {}", data.invite_code, e);
        DefaultApiError::Internal
    })??;

    if invite.active {
        if let Some(expiration) = invite.expiration {
            if expiration <= chrono::Utc::now() {
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
