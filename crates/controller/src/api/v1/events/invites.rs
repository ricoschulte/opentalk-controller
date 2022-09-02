use super::{ApiResponse, DefaultApiResult, PagePaginationQuery};
use crate::api::v1::events::{
    enrich_invitees_from_keycloak, EventInvitee, EventPoliciesBuilderExt,
};
use crate::api::v1::response::{ApiError, Created, NoContent};
use crate::api::v1::rooms::RoomsPoliciesBuilderExt;
use crate::services::MailService;
use crate::settings::SharedSettingsActix;
use actix_web::web::{Data, Json, Path, Query, ReqData};
use actix_web::{delete, get, patch, post, Either};
use anyhow::Context;
use database::Db;
use db_storage::events::email_invites::{EventEmailInvite, NewEventEmailInvite};
use db_storage::events::{
    Event, EventFavorite, EventId, EventInvite, EventInviteStatus, NewEventInvite,
    UpdateEventInvite,
};
use db_storage::invites::NewInvite;
use db_storage::rooms::Room;
use db_storage::sip_configs::SipConfig;
use db_storage::users::{User, UserId};
use diesel::Connection;
use email_address::EmailAddress;
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
    kc_admin_client: Data<KeycloakAdminClient>,
    event_id: Path<EventId>,
    pagination: Query<PagePaginationQuery>,
) -> DefaultApiResult<Vec<EventInvitee>> {
    let settings = settings.load_full();
    let event_id = event_id.into_inner();
    let PagePaginationQuery { per_page, page } = pagination.into_inner();

    let (invitees, invitees_total) = crate::block(move || -> database::Result<_> {
        let conn = db.get_conn()?;

        // FIXME: Preliminary solution, consider using UNION when Diesel supports it.
        // As in #[get("/events")], we simply get all invitees and truncate them afterwards.
        // Note that get_for_event_paginated returns a total record count of 0 when paging beyond the end.

        let (event_invites_with_user, event_invites_total) =
            EventInvite::get_for_event_paginated(&conn, event_id, i64::max_value(), 1)?;

        let event_invitees_iter =
            event_invites_with_user
                .into_iter()
                .map(|(event_invite, user)| {
                    EventInvitee::from_invite_with_user(event_invite, user, &settings)
                });

        let (event_email_invites, event_email_invites_total) =
            EventEmailInvite::get_for_event_paginated(&conn, event_id, i64::max_value(), 1)?;

        let event_email_invitees_iter = event_email_invites.into_iter().map(|event_email_invite| {
            EventInvitee::from_email_invite(event_email_invite, &settings)
        });

        let invitees_to_skip_count = (page - 1) * per_page;
        let invitees = event_invitees_iter
            .chain(event_email_invitees_iter)
            .skip(invitees_to_skip_count as usize)
            .take(per_page as usize)
            .collect();

        Ok((invitees, event_invites_total + event_email_invites_total))
    })
    .await??;

    let invitees = enrich_invitees_from_keycloak(&kc_admin_client, invitees).await;

    Ok(ApiResponse::new(invitees).with_page_pagination(per_page, page, invitees_total))
}

/// Request body for the `POST /events/{event_id}/invites` endpoint
#[derive(Deserialize)]
#[serde(untagged)]
pub enum PostEventInviteBody {
    User { invitee: UserId },
    Email { email: EmailAddress },
}

/// API Endpoint `POST /events/{event_id}/invites`
///
/// Invite a user to an event
#[post("/events/{event_id}/invites")]
#[allow(clippy::too_many_arguments)]
pub async fn create_invite_to_event(
    settings: SharedSettingsActix,
    db: Data<Db>,
    authz: Data<Authz>,
    kc_admin_client: Data<KeycloakAdminClient>,
    current_user: ReqData<User>,
    event_id: Path<EventId>,
    create_invite: Json<PostEventInviteBody>,
    mail_service: Data<MailService>,
) -> Result<Either<Created, NoContent>, ApiError> {
    let event_id = event_id.into_inner();

    match create_invite.into_inner() {
        PostEventInviteBody::User { invitee } => {
            create_user_event_invite(
                db,
                authz,
                current_user.into_inner(),
                event_id,
                invitee,
                &mail_service.into_inner(),
            )
            .await
        }
        PostEventInviteBody::Email { email } => {
            create_email_event_invite(
                settings,
                db,
                authz,
                kc_admin_client,
                current_user.into_inner(),
                event_id,
                email,
                &mail_service.into_inner(),
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
    invitee_id: UserId,
    mail_service: &MailService,
) -> Result<Either<Created, NoContent>, ApiError> {
    let inviter = current_user.clone();

    let res = crate::block(move || -> database::Result<Either<_, NoContent>> {
        let conn = db.get_conn()?;

        let (event, room, sip_config) = Event::get_with_room(&conn, event_id)?;
        let invitee = User::get(&conn, invitee_id)?;

        if event.created_by == invitee_id {
            return Ok(Either::Right(NoContent));
        }

        let res = NewEventInvite {
            event_id,
            invitee: invitee_id,
            created_by: current_user.id,
            created_at: None,
        }
        .try_insert(&conn);

        match res {
            Ok(Some(_invite)) => Ok(Either::Left((event, room, sip_config, invitee))),
            Ok(None) => Ok(Either::Right(NoContent)),
            Err(e) => Err(e),
        }
    })
    .await??;

    match res {
        Either::Left((event, room, sip_config, invitee)) => {
            let policies = PoliciesBuilder::new()
                // Grant invitee access
                .grant_user_access(invitee.id)
                .event_read_access(event_id)
                .room_read_access(event.room)
                .event_invite_invitee_access(event_id)
                .finish();

            authz.add_policies(policies).await?;

            mail_service
                .send_registered_invite(inviter, event, room, sip_config, invitee)
                .await
                .context("Failed to send with MailService")?;

            Ok(Either::Left(Created))
        }
        Either::Right(response) => Ok(Either::Right(response)),
    }
}

/// Create an invite to an event via email address
///
/// Checks first if a user exists with the email address in our database and creates a regular invite
/// else checks if the email is registered with the keycloak and then creates an email invite
#[allow(clippy::too_many_arguments)]
async fn create_email_event_invite(
    settings: SharedSettingsActix,
    db: Data<Db>,
    authz: Data<Authz>,
    kc_admin_client: Data<KeycloakAdminClient>,
    current_user: User,
    event_id: EventId,
    email: EmailAddress,
    mail_service: &MailService,
) -> Result<Either<Created, NoContent>, ApiError> {
    #[allow(clippy::large_enum_variant)]
    enum UserState {
        ExistsAndIsAlreadyInvited,
        ExistsAndWasInvited {
            event: Event,
            room: Room,
            invitee: User,
            sip_config: Option<SipConfig>,
            invite: EventInvite,
        },
        DoesNotExist {
            event: Event,
            room: Room,
            sip_config: Option<SipConfig>,
        },
    }

    let state = {
        let email = email.clone();
        let current_user = current_user.clone();
        let db = db.clone();

        crate::block(move || -> Result<_, ApiError> {
            let conn = db.get_conn()?;

            let (event, room, sip_config) = Event::get_with_room(&conn, event_id)?;

            let invitee_user =
                User::get_by_email(&conn, &current_user.oidc_issuer, email.as_ref())?;

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
                .try_insert(&conn);

                match res {
                    Ok(Some(invite)) => Ok(UserState::ExistsAndWasInvited {
                        event,
                        room,
                        invitee: invitee_user,
                        sip_config,
                        invite,
                    }),
                    Ok(None) => Ok(UserState::ExistsAndIsAlreadyInvited),
                    Err(e) => Err(e.into()),
                }
            } else {
                Ok(UserState::DoesNotExist {
                    event,
                    room,
                    sip_config,
                })
            }
        })
        .await??
    };

    match state {
        UserState::ExistsAndIsAlreadyInvited => Ok(Either::Right(NoContent)),
        UserState::ExistsAndWasInvited {
            event,
            room,
            invite,
            sip_config,
            invitee,
        } => {
            let policies = PoliciesBuilder::new()
                // Grant invitee access
                .grant_user_access(invite.invitee)
                .event_read_access(event_id)
                .room_read_access(room.id)
                .event_invite_invitee_access(event_id)
                .finish();

            authz.add_policies(policies).await?;

            mail_service
                .send_registered_invite(current_user, event, room, sip_config, invitee)
                .await
                .context("Failed to send with MailService")?;

            Ok(Either::Left(Created))
        }
        UserState::DoesNotExist {
            event,
            room,
            sip_config,
        } => {
            create_invite_to_non_matching_email(
                settings,
                db,
                kc_admin_client,
                mail_service,
                current_user,
                event,
                room,
                sip_config,
                email,
            )
            .await
        }
    }
}

/// Invite a given email to the event
/// Will check if the email exists in keycloak and sends an "unregistered" email invite
/// or (if configured) sends an "external" email invite to the given email address
#[allow(clippy::too_many_arguments)]
async fn create_invite_to_non_matching_email(
    settings: SharedSettingsActix,
    db: Data<Db>,
    kc_admin_client: Data<KeycloakAdminClient>,
    mail_service: &MailService,
    current_user: User,
    event: Event,
    room: Room,
    sip_config: Option<SipConfig>,
    email: EmailAddress,
) -> Result<Either<Created, NoContent>, ApiError> {
    let email_exists = kc_admin_client
        .verify_email(email.as_ref())
        .await
        .context("Failed to verify email")?;

    if email_exists
        || settings
            .load()
            .endpoints
            .event_invite_external_email_address
    {
        let inviter = current_user.clone();
        let invitee = email.clone();

        let res = {
            let db = db.clone();
            let event_id = event.id;
            let current_user_id = current_user.id;

            crate::block(move || {
                let conn = db.get_conn()?;

                NewEventEmailInvite {
                    event_id,
                    email: email.into(),
                    created_by: current_user_id,
                }
                .try_insert(&conn)
            })
            .await?
        };

        match res {
            Ok(Some(_)) => {
                if email_exists {
                    mail_service
                        .send_unregistered_invite(
                            inviter,
                            event,
                            room,
                            sip_config,
                            invitee.as_ref(),
                        )
                        .await
                        .context("Failed to send with MailService")?;
                } else {
                    let room_id = room.id;

                    let invite = crate::block(move || {
                        let conn = db.get_conn()?;

                        NewInvite {
                            active: true,
                            created_by: current_user.id,
                            updated_by: current_user.id,
                            room: room_id,
                            expiration: None,
                        }
                        .insert(&conn)
                    })
                    .await??;

                    mail_service
                        .send_external_invite(
                            inviter,
                            event,
                            room,
                            sip_config,
                            invitee.as_ref(),
                            invite.id.to_string(),
                        )
                        .await
                        .context("Failed to send with MailService")?;
                }

                Ok(Either::Left(Created))
            }
            Ok(None) => Ok(Either::Right(NoContent)),
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
