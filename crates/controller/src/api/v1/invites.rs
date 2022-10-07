//! Contains invite related REST endpoints.
use super::response::{ApiError, NoContent};
use super::DefaultApiResult;
use crate::api::v1::users::PublicUserProfile;
use crate::api::v1::{ApiResponse, PagePaginationQuery};
use crate::settings::SharedSettingsActix;
use actix_web::web::{Data, Json, Path, Query, ReqData};
use actix_web::{delete, get, post, put};
use chrono::{DateTime, Utc};
use database::{DatabaseError, Db};
use db_storage::invites::{Invite, InviteCodeId, NewInvite, UpdateInvite};
use db_storage::rooms::{Room, RoomId};
use db_storage::users::User;
use serde::{Deserialize, Serialize};
use validator::Validate;

/// Public invite details
///
/// Contains general public information about a room.
#[derive(Debug, Serialize)]
pub struct InviteResource {
    pub invite_code: InviteCodeId,
    pub created: DateTime<Utc>,
    pub created_by: PublicUserProfile,
    pub updated: DateTime<Utc>,
    pub updated_by: PublicUserProfile,
    pub room_id: RoomId,
    pub active: bool,
    pub expiration: Option<DateTime<Utc>>,
}

impl InviteResource {
    fn from_with_user(
        val: Invite,
        created_by: PublicUserProfile,
        updated_by: PublicUserProfile,
    ) -> Self {
        InviteResource {
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
pub struct PostInviteBody {
    pub expiration: Option<DateTime<Utc>>,
}

/// API Endpoint *POST /rooms/{room_id}/invites*
///
/// Uses the provided [`NewInvite`] to create a new invite.
#[post("/rooms/{room_id}/invites")]
pub async fn add_invite(
    settings: SharedSettingsActix,
    db: Data<Db>,
    current_user: ReqData<User>,
    room_id: Path<RoomId>,
    data: Json<PostInviteBody>,
) -> DefaultApiResult<InviteResource> {
    let settings = settings.load_full();
    let room_id = room_id.into_inner();
    let current_user = current_user.into_inner();

    let new_invite = data.into_inner();
    let current_user_clone = current_user.clone();
    let db_invite = crate::block(move || {
        let mut conn = db.get_conn()?;

        let new_invite = NewInvite {
            active: true,
            created_by: current_user_clone.id,
            updated_by: current_user_clone.id,
            room: room_id,
            expiration: new_invite.expiration,
        };

        new_invite.insert(&mut conn)
    })
    .await??;

    let created_by = PublicUserProfile::from_db(&settings, current_user.clone());
    let updated_by = PublicUserProfile::from_db(&settings, current_user);

    let invite = InviteResource::from_with_user(db_invite, created_by, updated_by);

    Ok(ApiResponse::new(invite))
}

/// API Endpoint *GET /rooms/{room_id}/invites*
///
/// Returns a JSON array of all accessible invites for the given room
#[get("/rooms/{room_id}/invites")]
pub async fn get_invites(
    settings: SharedSettingsActix,
    db: Data<Db>,
    room_id: Path<RoomId>,
    pagination: Query<PagePaginationQuery>,
) -> DefaultApiResult<Vec<InviteResource>> {
    let settings = settings.load_full();
    let room_id = room_id.into_inner();
    let PagePaginationQuery { per_page, page } = pagination.into_inner();

    let (invites_with_users, total_invites) = crate::block(move || {
        let mut conn = db.get_conn()?;

        let room = Room::get(&mut conn, room_id)?;

        Invite::get_all_for_room_with_users_paginated(&mut conn, room.id, per_page, page)
    })
    .await??;

    let invites = invites_with_users
        .into_iter()
        .map(|(db_invite, created_by, updated_by)| {
            let created_by = PublicUserProfile::from_db(&settings, created_by);
            let updated_by = PublicUserProfile::from_db(&settings, updated_by);

            InviteResource::from_with_user(db_invite, created_by, updated_by)
        })
        .collect::<Vec<InviteResource>>();

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
    path_params: Path<RoomIdAndInviteCode>,
) -> DefaultApiResult<InviteResource> {
    let settings = settings.load_full();

    let RoomIdAndInviteCode {
        room_id,
        invite_code,
    } = path_params.into_inner();

    let (db_invite, created_by, updated_by) = crate::block(move || {
        let mut conn = db.get_conn()?;

        let invite_with_users = Invite::get_with_users(&mut conn, invite_code)?;

        if invite_with_users.0.room != room_id {
            return Err(DatabaseError::NotFound);
        }

        Ok(invite_with_users)
    })
    .await??;

    let created_by = PublicUserProfile::from_db(&settings, created_by);
    let updated_by = PublicUserProfile::from_db(&settings, updated_by);

    Ok(ApiResponse::new(InviteResource::from_with_user(
        db_invite, created_by, updated_by,
    )))
}

/// Body for *PUT /rooms/{room_id}/invites/{invite_code}*
#[derive(Debug, Deserialize)]
pub struct PutInviteBody {
    pub expiration: Option<DateTime<Utc>>,
}

/// API Endpoint *PUT /rooms/{room_id}/invites/{invite_code}*
///
/// Uses the provided [`PutInviteBody`] to modify a specified invite.
/// Returns the modified [`InviteResource`]
#[put("/rooms/{room_id}/invites/{invite_code}")]
pub async fn update_invite(
    settings: SharedSettingsActix,
    db: Data<Db>,
    current_user: ReqData<User>,
    path_params: Path<RoomIdAndInviteCode>,
    update_invite: Json<PutInviteBody>,
) -> DefaultApiResult<InviteResource> {
    let settings = settings.load_full();
    let current_user = current_user.into_inner();
    let RoomIdAndInviteCode {
        room_id,
        invite_code,
    } = path_params.into_inner();
    let update_invite = update_invite.into_inner();

    let current_user_id = current_user.id;
    let (invite, created_by) = crate::block(move || {
        let mut conn = db.get_conn()?;

        let invite = Invite::get(&mut conn, invite_code)?;

        if invite.room != room_id {
            return Err(DatabaseError::NotFound);
        }

        let created_by = User::get(&mut conn, invite.created_by)?;

        let now = chrono::Utc::now();
        let changeset = UpdateInvite {
            updated_by: Some(current_user_id),
            updated_at: Some(now),
            expiration: Some(update_invite.expiration),
            active: None,
            room: None,
        };

        let invite = changeset.apply(&mut conn, room_id, invite_code)?;

        Ok((invite, created_by))
    })
    .await??;

    let created_by = PublicUserProfile::from_db(&settings, created_by);
    let updated_by = PublicUserProfile::from_db(&settings, current_user);

    Ok(ApiResponse::new(InviteResource::from_with_user(
        invite, created_by, updated_by,
    )))
}

/// API Endpoint *PUT /rooms/{room_id}*
///
/// Deletes the [`Invite`] identified by this resource.
/// Returns 204 No Content
#[delete("/rooms/{room_id}/invites/{invite_code}")]
pub async fn delete_invite(
    db: Data<Db>,
    current_user: ReqData<User>,
    path_params: Path<RoomIdAndInviteCode>,
) -> Result<NoContent, ApiError> {
    let RoomIdAndInviteCode {
        room_id,
        invite_code,
    } = path_params.into_inner();

    crate::block(move || {
        let mut conn = db.get_conn()?;

        let changeset = UpdateInvite {
            updated_by: Some(current_user.id),
            updated_at: Some(Utc::now()),
            expiration: None,
            active: Some(false),
            room: None,
        };

        changeset.apply(&mut conn, room_id, invite_code)
    })
    .await??;

    Ok(NoContent)
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

    data.validate()?;

    let invite = crate::block(move || -> database::Result<_> {
        let mut conn = db.get_conn()?;

        Invite::get(&mut conn, data.invite_code)
    })
    .await??;

    if invite.active {
        if let Some(expiration) = invite.expiration {
            if expiration <= Utc::now() {
                // Do not leak the existence of the invite when it is expired
                return Err(ApiError::not_found());
            }
        }
        Ok(ApiResponse::new(CodeVerified {
            room_id: invite.room,
        }))
    } else {
        // TODO(r.floren) Do we want to return something else here?
        Err(ApiError::not_found())
    }
}
