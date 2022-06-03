use super::{ApiResponse, DefaultApiResult, PagePaginationQuery};
use crate::api::v1::events::{EventInvitee, EventPoliciesBuilderExt};
use crate::api::v1::response::{ApiError, Created, NoContent};
use crate::api::v1::rooms::RoomsPoliciesBuilderExt;
use crate::api::v1::users::PublicUserProfile;
use crate::settings::SharedSettingsActix;
use actix_web::web::{Data, Json, Path, Query, ReqData};
use actix_web::{delete, get, patch, post, Either};
use database::{Db, OptionalExt};
use db_storage::events::email_invites::NewEventEmailInvite;
use db_storage::events::{
    Event, EventFavorite, EventId, EventInvite, EventInviteStatus, NewEventInvite,
    UpdateEventInvite,
};
use db_storage::rooms::RoomId;
use db_storage::users::{User, UserId};
use diesel::Connection;
use keycloak_admin::KeycloakAdminClient;
use kustos::policies_builder::PoliciesBuilder;
use kustos::Authz;
use serde::{Deserialize, Serialize};

/// API Endpoint `GET /events/{event_id}/invites`
///
/// Get all invites for an event
#[get("/events/{event_id}/invites")]
pub async fn get_invites_for_event(
    settings: SharedSettingsActix,
    db: Data<Db>,
    event_id: Path<EventId>,
    pagination: Query<PagePaginationQuery>,
) -> DefaultApiResult<Vec<EventInvitee>> {
    let settings = settings.load_full();
    let PagePaginationQuery { per_page, page } = pagination.into_inner();

    let (event_invitees, total_event_invitees) = crate::block(move || -> database::Result<_> {
        let conn = db.get_conn()?;

        let (event_invites_with_invitee, total) =
            EventInvite::get_for_event_paginated(&conn, event_id.into_inner(), per_page, page)?;

        let mut event_invitees = vec![];

        for (event_invite, invitee) in event_invites_with_invitee {
            event_invitees.push(EventInvitee {
                profile: PublicUserProfile::from_db(&settings, invitee),
                status: event_invite.status,
            });
        }

        Ok((event_invitees, total))
    })
    .await??;

    Ok(ApiResponse::new(event_invitees).with_page_pagination(per_page, page, total_event_invitees))
}

/// Request body for the `POST /events/{event_id}/invites` endpoint
#[derive(Deserialize)]
#[serde(untagged)]
pub enum PostEventInviteBody {
    User { invitee: UserId },
    Email { email: String },
}

/// API Endpoint `POST /events/{event_id}/invites`
///
/// Invite a user to an event
#[post("/events/{event_id}/invites")]
pub async fn create_invite_to_event(
    db: Data<Db>,
    authz: Data<Authz>,
    kc_admin_client: Data<KeycloakAdminClient>,
    current_user: ReqData<User>,
    event_id: Path<EventId>,
    create_invite: Json<PostEventInviteBody>,
) -> Result<Either<Created, NoContent>, ApiError> {
    let event_id = event_id.into_inner();

    match create_invite.into_inner() {
        PostEventInviteBody::User { invitee } => {
            create_user_event_invite(db, authz, current_user.into_inner(), event_id, invitee).await
        }
        PostEventInviteBody::Email { email } => {
            create_email_event_invite(
                db,
                authz,
                kc_admin_client,
                current_user.into_inner(),
                event_id,
                email,
            )
            .await
        }
    }
}

async fn create_user_event_invite(
    db: Data<Db>,
    authz: Data<Authz>,
    current_user: User,
    event_id: EventId,
    invitee: UserId,
) -> Result<Either<Created, NoContent>, ApiError> {
    let res = crate::block(move || -> database::Result<Either<_, NoContent>> {
        let conn = db.get_conn()?;

        let event = Event::get(&conn, event_id)?;

        if event.created_by == invitee {
            return Ok(Either::Right(NoContent));
        }

        let res = NewEventInvite {
            event_id,
            invitee,
            created_by: current_user.id,
            created_at: None,
        }
        .insert(&conn);

        match res {
            Ok(invite) => Ok(Either::Left((event.room, invite))),
            Err(database::DatabaseError::DieselError(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                ..,
            ))) => Ok(Either::Right(NoContent)),
            Err(e) => Err(e),
        }
    })
    .await??;

    match res {
        Either::Left((room_id, invite)) => {
            let policies = PoliciesBuilder::new()
                // Grant invitee access
                .grant_user_access(invite.invitee)
                .event_read_access(event_id)
                .room_read_access(room_id)
                .event_invite_invitee_access(event_id)
                .finish();

            authz.add_policies(policies).await?;

            Ok(Either::Left(Created))
        }
        Either::Right(response) => Ok(Either::Right(response)),
    }
}

/// Create an invite to an event via email address
///
/// Checks first if a user exists with the email address in our database and creates a regular invite
/// else checks if the email is registered with the keycloak and then creates an email invite
async fn create_email_event_invite(
    db: Data<Db>,
    authz: Data<Authz>,
    kc_admin_client: Data<KeycloakAdminClient>,
    current_user: User,
    event_id: EventId,
    email: String,
) -> Result<Either<Created, NoContent>, ApiError> {
    enum UserState {
        ExistsAndIsAlreadyInvited,
        ExistsAndWasInvited(RoomId, EventInvite),
        DoesNotExist,
    }

    let state = {
        let email = email.clone();
        let current_user = current_user.clone();
        let db = db.clone();

        crate::block(move || -> Result<_, ApiError> {
            let conn = db.get_conn()?;

            let event = Event::get(&conn, event_id)?;

            let invitee_user =
                User::get_by_email(&conn, &current_user.oidc_issuer, &email).optional()?;

            if let Some(invitee_user) = invitee_user {
                if event.created_by == invitee_user.id {
                    return Ok(UserState::ExistsAndIsAlreadyInvited);
                }

                let res = NewEventInvite {
                    event_id,
                    invitee: invitee_user.id,
                    created_by: current_user.id,
                    created_at: None,
                }
                .insert(&conn);

                match res {
                    Ok(invite) => Ok(UserState::ExistsAndWasInvited(event.room, invite)),
                    Err(database::DatabaseError::DieselError(
                        diesel::result::Error::DatabaseError(
                            diesel::result::DatabaseErrorKind::UniqueViolation,
                            ..,
                        ),
                    )) => Ok(UserState::ExistsAndIsAlreadyInvited),
                    Err(e) => Err(e.into()),
                }
            } else {
                Ok(UserState::DoesNotExist)
            }
        })
        .await??
    };

    match state {
        UserState::ExistsAndIsAlreadyInvited => Ok(Either::Right(NoContent)),
        UserState::ExistsAndWasInvited(room_id, invite) => {
            let policies = PoliciesBuilder::new()
                // Grant invitee access
                .grant_user_access(invite.invitee)
                .event_read_access(event_id)
                .room_read_access(room_id)
                .event_invite_invitee_access(event_id)
                .finish();

            authz.add_policies(policies).await?;

            Ok(Either::Left(Created))
        }
        UserState::DoesNotExist => {
            let email_exists = kc_admin_client
                .verify_email(&email)
                .await
                .map_err(anyhow::Error::from)?;

            if email_exists {
                // TODO: send email

                let res = crate::block(move || {
                    let conn = db.get_conn()?;

                    NewEventEmailInvite {
                        event_id,
                        email,
                        created_by: current_user.id,
                    }
                    .insert(&conn)
                })
                .await?;

                match res {
                    Ok(()) => Ok(Either::Left(Created)),
                    Err(database::DatabaseError::DieselError(
                        diesel::result::Error::DatabaseError(
                            diesel::result::DatabaseErrorKind::UniqueViolation,
                            ..,
                        ),
                    )) => Ok(Either::Right(NoContent)),
                    Err(e) => Err(e.into()),
                }
            } else {
                Err(ApiError::conflict()
                    .with_code("unknown_email")
                    .with_message(
                    "Only emails registered with the systems are allowed to be used for invites",
                ))
            }
        }
    }
}

/// Path parameters for the `DELETE /events/{event_id}/invites/{invite_id}` endpoint
#[derive(Deserialize)]
pub struct DeleteEventInvitePath {
    pub event_id: EventId,
    pub user_id: UserId,
}

/// API Endpoint `DELETE /events/{event_id}/invites/{invite_id}`
///
/// Delete/Withdraw an event invitation made to a user
#[delete("/events/{event_id}/invites/{user_id}")]
pub async fn delete_invite_to_event(
    db: Data<Db>,
    authz: Data<Authz>,
    current_user: ReqData<User>,
    path_params: Path<DeleteEventInvitePath>,
) -> Result<NoContent, ApiError> {
    let DeleteEventInvitePath { event_id, user_id } = path_params.into_inner();

    let (room_id, invite) = crate::block(move || -> database::Result<_> {
        let conn = db.get_conn()?;

        conn.transaction(|| {
            // delete invite to the event
            let invite = EventInvite::delete_by_invitee(&conn, event_id, user_id)?;

            // user access is going to be removed for the event, remove favorite entry if it exists
            EventFavorite::delete_by_id(&conn, current_user.id, event_id)?;

            let event = Event::get(&conn, invite.event_id)?;

            Ok((event.room, invite))
        })
    })
    .await??;

    let resources = vec![
        format!("/events/{event_id}"),
        format!("/events/{event_id}/instances"),
        format!("/events/{event_id}/instances/*"),
        format!("/events/{event_id}/invites"),
        format!("/users/me/event_favorites/{event_id}"),
        format!("/events/{event_id}/invite"),
        format!("/rooms/{room_id}"),
        format!("/rooms/{room_id}/invites"),
        format!("/rooms/{room_id}/start"),
    ];

    authz
        .remove_all_user_permission_for_resources(invite.invitee, resources)
        .await?;

    Ok(NoContent)
}

/// Response body for the `GET /event_invites/pending` endpoint
#[derive(Serialize)]
pub struct GetEventInvitesPendingResponse {
    total_pending_invites: u32,
}

/// API Endpoint `GET /users/me/pending_invites`
#[get("/users/me/pending_invites")]
pub async fn get_event_invites_pending(
    db: Data<Db>,
    current_user: ReqData<User>,
) -> DefaultApiResult<GetEventInvitesPendingResponse> {
    let event_invites = crate::block(move || -> database::Result<_> {
        let conn = db.get_conn()?;

        EventInvite::get_pending_for_user(&conn, current_user.id)
    })
    .await??;

    Ok(ApiResponse::new(GetEventInvitesPendingResponse {
        total_pending_invites: event_invites.len() as u32,
    }))
}

/// API Endpoint `PATCH /events/{event_id}/invite`
///
/// Accept an invite to an event
#[patch("/events/{event_id}/invite")]
pub async fn accept_event_invite(
    db: Data<Db>,
    current_user: ReqData<User>,
    event_id: Path<EventId>,
) -> Result<NoContent, ApiError> {
    let event_id = event_id.into_inner();

    crate::block(move || -> database::Result<_> {
        let conn = db.get_conn()?;

        let changeset = UpdateEventInvite {
            status: EventInviteStatus::Accepted,
        };

        changeset.apply(&conn, current_user.id, event_id)
    })
    .await??;

    Ok(NoContent)
}

/// API Endpoint `DELETE /events/{event_id}/invite`
///
/// Decline an invite to an event
#[delete("/events/{event_id}/invite")]
pub async fn decline_event_invite(
    db: Data<Db>,
    current_user: ReqData<User>,
    event_id: Path<EventId>,
) -> Result<NoContent, ApiError> {
    let event_id = event_id.into_inner();

    crate::block(move || -> database::Result<_> {
        let conn = db.get_conn()?;

        let changeset = UpdateEventInvite {
            status: EventInviteStatus::Declined,
        };

        changeset.apply(&conn, current_user.id, event_id)
    })
    .await??;

    Ok(NoContent)
}
