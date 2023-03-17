// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use super::{
    can_edit, ApiResponse, DateTimeTz, DefaultApiResult, EventAndInstanceId, EventInvitee,
    EventRoomInfo, EventStatus, EventType, InstanceId, LOCAL_DT_FORMAT,
};
use crate::api::v1::cursor::Cursor;
use crate::api::v1::events::enrich_invitees_from_keycloak;
use crate::api::v1::response::{ApiError, NoContent};
use crate::api::v1::users::PublicUserProfile;
use crate::api::v1::util::{GetUserProfilesBatched, UserProfilesBatch};
use crate::settings::SharedSettingsActix;
use actix_web::web::{Data, Json, Path, Query, ReqData};
use actix_web::{get, patch, Either};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use database::Db;
use db_storage::events::{
    Event, EventException, EventExceptionKind, EventId, EventInviteStatus, NewEventException,
    UpdateEventException,
};
use db_storage::tenants::Tenant;
use db_storage::users::User;
use keycloak_admin::KeycloakAdminClient;
use rrule::RRuleSet;
use serde::{Deserialize, Serialize};
use validator::Validate;

/// Event instance resource
///
/// An event instance is an occurrence of an recurring event
///
/// Exceptions for the instance are always already applied
///
/// For infos on undocumented fields see [`EventResource`](super::EventResource)
#[derive(Debug, Serialize)]
pub struct EventInstance {
    /// Opaque id of the event instance resource
    pub id: EventAndInstanceId,

    /// ID of the recurring event this instance belongs to
    pub recurring_event_id: EventId,

    /// Opaque id of the instance
    pub instance_id: InstanceId,

    /// Public user profile of the user which created the event
    pub created_by: PublicUserProfile,

    /// Timestamp of the event creation
    pub created_at: DateTime<Utc>,

    /// Public user profile of the user which last updated the event
    /// or created the exception which modified the instance
    pub updated_by: PublicUserProfile,

    /// Timestamp of the last update
    pub updated_at: DateTime<Utc>,

    pub title: String,
    pub description: String,
    pub room: EventRoomInfo,
    pub invitees_truncated: bool,
    pub invitees: Vec<EventInvitee>,
    pub is_all_day: bool,
    pub starts_at: DateTimeTz,
    pub ends_at: DateTimeTz,

    /// Must always be `instance`
    #[serde(rename = "type")]
    pub type_: EventType,
    pub status: EventStatus,
    pub invite_status: EventInviteStatus,
    pub is_favorite: bool,
    pub can_edit: bool,
}

#[derive(Deserialize)]
pub struct GetEventInstancesQuery {
    #[serde(default)]
    invitees_max: i64,
    time_min: Option<DateTime<Utc>>,
    time_max: Option<DateTime<Utc>>,
    per_page: Option<i64>,
    after: Option<Cursor<GetEventInstancesCursorData>>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct GetEventInstancesCursorData {
    page: i64,
}

struct GetPaginatedEventInstancesData {
    instances: Vec<EventInstance>,
    before: Option<String>,
    after: Option<String>,
}

#[get("/events/{event_id}/instances")]
pub async fn get_event_instances(
    settings: SharedSettingsActix,
    db: Data<Db>,
    kc_admin_client: Data<KeycloakAdminClient>,
    current_tenant: ReqData<Tenant>,
    current_user: ReqData<User>,
    event_id: Path<EventId>,
    query: Query<GetEventInstancesQuery>,
) -> DefaultApiResult<Vec<EventInstance>> {
    let settings = settings.load_full();
    let event_id = event_id.into_inner();
    let GetEventInstancesQuery {
        invitees_max,
        time_min,
        time_max,
        per_page,
        after,
    } = query.into_inner();

    let per_page = per_page.unwrap_or(30).clamp(1, 100);
    let page = after.map(|c| c.page).unwrap_or(1).max(1);

    let skip = per_page as usize;
    let offset = (page - 1) as usize;

    let kc_admin_client_ref = &kc_admin_client;

    let instances_data = crate::block(
        move || -> Result<GetPaginatedEventInstancesData, ApiError> {
            let mut conn = db.get_conn()?;

            let (event, invite, room, sip_config, is_favorite) =
                Event::get_with_invite_and_room(&mut conn, current_user.id, event_id)?;

            let (invitees, invitees_truncated) =
                super::get_invitees_for_event(&settings, &mut conn, event.id, invitees_max)?;

            let invite_status = invite
                .map(|inv| inv.status)
                .unwrap_or(EventInviteStatus::Accepted);

            let rruleset = build_rruleset(&event)?;

            // limit of how far into the future we calculate instances (40 years)
            let max_dt = Utc::now().with_timezone(&rruleset.dt_start.timezone())
                + chrono::Duration::weeks(12 * 40);

            let mut iter: Box<dyn Iterator<Item = DateTime<Tz>>> =
                Box::new(rruleset.into_iter().skip_while(move |&dt| dt > max_dt));

            if let Some(time_min) = time_min {
                iter = Box::new(iter.skip_while(move |&dt| dt <= time_min));
            }

            if let Some(time_max) = time_max {
                iter = Box::new(iter.skip_while(move |&dt| dt >= time_max));
            }

            let datetimes: Vec<DateTime<Utc>> = iter
                .skip(skip * offset)
                .take(skip)
                .map(|dt| dt.with_timezone(&Utc))
                .collect();

            let exceptions = EventException::get_all_for_event(&mut conn, event_id, &datetimes)?;

            let users = GetUserProfilesBatched::new()
                .add(&event)
                .add(&exceptions)
                .fetch(&settings, &mut conn)?;

            let room = EventRoomInfo::from_room(&settings, room, sip_config);

            let can_edit = can_edit(&event, &current_user);

            let mut exceptions = exceptions.into_iter().peekable();

            let mut instances = vec![];

            for datetime in datetimes {
                let exception =
                    exceptions.next_if(|exception| exception.exception_date == datetime);

                let instance = create_event_instance(
                    &users,
                    event.clone(),
                    invite_status,
                    is_favorite,
                    exception,
                    room.clone(),
                    InstanceId(datetime),
                    invitees.clone(),
                    invitees_truncated,
                    can_edit,
                )?;

                instances.push(instance);
            }

            let next_cursor = if !instances.is_empty() {
                Some(Cursor(GetEventInstancesCursorData { page: page + 1 }).to_base64())
            } else {
                None
            };

            Ok(GetPaginatedEventInstancesData {
                instances,
                before: None,
                after: next_cursor,
            })
        },
    )
    .await??;

    // Enrich the invitees for the first instance only and reuse them as all instances have the same invitees.
    let event_instances = if let Some(instance) = instances_data.instances.first() {
        let enriched_invitees = enrich_invitees_from_keycloak(
            kc_admin_client_ref,
            &current_tenant,
            instance.invitees.clone(),
        )
        .await;

        instances_data
            .instances
            .into_iter()
            .map(|instance| EventInstance {
                invitees: enriched_invitees.clone(),
                ..instance
            })
            .collect()
    } else {
        instances_data.instances
    };

    Ok(ApiResponse::new(event_instances)
        .with_cursor_pagination(instances_data.before, instances_data.after))
}

#[derive(Deserialize)]
pub struct GetEventInstancePath {
    event_id: EventId,
    instance_id: InstanceId,
}

#[derive(Deserialize)]
pub struct GetEventInstanceQuery {
    #[serde(default)]
    invitees_max: i64,
}

/// API Endpoint *GET /events/{id}*
///
/// Returns the event resource for the given id
#[get("/events/{event_id}/instances/{instance_id}")]
pub async fn get_event_instance(
    settings: SharedSettingsActix,
    db: Data<Db>,
    kc_admin_client: Data<KeycloakAdminClient>,
    current_tenant: ReqData<Tenant>,
    current_user: ReqData<User>,
    path: Path<GetEventInstancePath>,
    query: Query<GetEventInstanceQuery>,
) -> DefaultApiResult<EventInstance> {
    let settings = settings.load_full();
    let GetEventInstancePath {
        event_id,
        instance_id,
    } = path.into_inner();
    let query = query.into_inner();

    let event_instance = crate::block(move || -> Result<EventInstance, ApiError> {
        let mut conn = db.get_conn()?;

        let (event, invite, room, sip_config, is_favorite) =
            Event::get_with_invite_and_room(&mut conn, current_user.id, event_id)?;
        verify_recurrence_date(&event, instance_id.0)?;

        let (invitees, invitees_truncated) =
            super::get_invitees_for_event(&settings, &mut conn, event_id, query.invitees_max)?;

        let exception = EventException::get_for_event(&mut conn, event_id, instance_id.0)?;

        let users = GetUserProfilesBatched::new()
            .add(&event)
            .add(&exception)
            .fetch(&settings, &mut conn)?;

        let room = EventRoomInfo::from_room(&settings, room, sip_config);

        let can_edit = can_edit(&event, &current_user);

        let event_instance = create_event_instance(
            &users,
            event,
            invite
                .map(|inv| inv.status)
                .unwrap_or(EventInviteStatus::Accepted),
            is_favorite,
            exception,
            room,
            instance_id,
            invitees,
            invitees_truncated,
            can_edit,
        )?;

        Ok(event_instance)
    })
    .await??;

    let event_instance = EventInstance {
        invitees: enrich_invitees_from_keycloak(
            &kc_admin_client,
            &current_tenant,
            event_instance.invitees,
        )
        .await,
        ..event_instance
    };

    Ok(ApiResponse::new(event_instance))
}

/// Path parameters for the `PATCH /events/{event_id}/{instance_id}` endpoint
#[derive(Deserialize)]
pub struct PatchEventInstancePath {
    event_id: EventId,
    instance_id: InstanceId,
}

/// Path query for the `PATCH /events/{event_id}/{instance_id}` endpoint
#[derive(Deserialize)]
pub struct PatchEventInstanceQuery {
    /// Maximum number of invitees to return inside the event instance resource
    ///
    /// Default: 0
    #[serde(default)]
    invitees_max: i64,
}

/// Request body for the `PATCH /events/{event_id}/{instance_id}` endpoint
#[derive(Debug, Deserialize, Validate)]
pub struct PatchEventInstanceBody {
    #[validate(length(max = 255))]
    title: Option<String>,
    #[validate(length(max = 4096))]
    description: Option<String>,
    is_all_day: Option<bool>,
    starts_at: Option<DateTimeTz>,
    ends_at: Option<DateTimeTz>,
    status: Option<EventStatus>,
}

impl PatchEventInstanceBody {
    fn is_empty(&self) -> bool {
        let PatchEventInstanceBody {
            title,
            description,
            is_all_day,
            starts_at,
            ends_at,
            status,
        } = self;

        title.is_none()
            && description.is_none()
            && is_all_day.is_none()
            && starts_at.is_none()
            && ends_at.is_none()
            && status.is_none()
    }
}

/// API Endpoint `PATCH /events/{event_id}/{instance_id}`
///
/// Patch an instance of an recurring event. This creates oder modifies an exception for the event
/// at the point of time of the given instance_id.
///
/// Returns the patched event instance
#[patch("/events/{event_id}/instances/{instance_id}")]
#[allow(clippy::too_many_arguments)]
pub async fn patch_event_instance(
    settings: SharedSettingsActix,
    db: Data<Db>,
    kc_admin_client: Data<KeycloakAdminClient>,
    current_tenant: ReqData<Tenant>,
    current_user: ReqData<User>,
    path: Path<PatchEventInstancePath>,
    query: Query<PatchEventInstanceQuery>,
    patch: Json<PatchEventInstanceBody>,
) -> Result<Either<ApiResponse<EventInstance>, NoContent>, ApiError> {
    let patch = patch.into_inner();

    if patch.is_empty() {
        return Ok(Either::Right(NoContent));
    }

    patch.validate()?;

    let settings = settings.load_full();
    let PatchEventInstancePath {
        event_id,
        instance_id,
    } = path.into_inner();

    let event_instance = crate::block(move || -> Result<EventInstance, ApiError> {
        let mut conn = db.get_conn()?;

        let (event, invite, room, sip_config, is_favorite) =
            Event::get_with_invite_and_room(&mut conn, current_user.id, event_id)?;

        if !event.is_recurring.unwrap_or_default() {
            return Err(ApiError::not_found());
        }

        verify_recurrence_date(&event, instance_id.0)?;

        let exception = if let Some(exception) =
            EventException::get_for_event(&mut conn, event_id, instance_id.0)?
        {
            let is_all_day = patch
                .is_all_day
                .or(exception.is_all_day)
                .or(event.is_all_day)
                .unwrap();
            let starts_at = patch
                .starts_at
                .or_else(|| DateTimeTz::starts_at_of(&event))
                .or_else(|| DateTimeTz::maybe_from_db(exception.starts_at, exception.starts_at_tz))
                .unwrap();
            let ends_at = patch
                .ends_at
                .or_else(|| DateTimeTz::ends_at_of(&event))
                .or_else(|| DateTimeTz::maybe_from_db(exception.ends_at, exception.ends_at_tz))
                .unwrap();

            super::verify_exception_dt_params(is_all_day, starts_at, ends_at)?;

            let update_exception = UpdateEventException {
                kind: match patch.status {
                    Some(EventStatus::Ok) => Some(EventExceptionKind::Modified),
                    Some(EventStatus::Cancelled) => Some(EventExceptionKind::Cancelled),
                    None => None,
                },
                title: patch.title.map(Some),
                description: patch.description.map(Some),
                is_all_day: patch.is_all_day.map(Some),
                starts_at: patch.starts_at.map(|dt| Some(dt.to_datetime_tz())),
                starts_at_tz: patch.starts_at.map(|dt| Some(dt.timezone)),
                ends_at: patch.ends_at.map(|dt| Some(dt.to_datetime_tz())),
                ends_at_tz: patch.ends_at.map(|dt| Some(dt.timezone)),
            };

            update_exception.apply(&mut conn, exception.id)?
        } else {
            let is_all_day = patch.is_all_day.or(event.is_all_day).unwrap();
            let starts_at = patch
                .starts_at
                .or_else(|| DateTimeTz::starts_at_of(&event))
                .unwrap();
            let ends_at = patch
                .ends_at
                .or_else(|| DateTimeTz::ends_at_of(&event))
                .unwrap();

            super::verify_exception_dt_params(is_all_day, starts_at, ends_at)?;

            let new_exception = NewEventException {
                event_id: event.id,
                exception_date: instance_id.0,
                exception_date_tz: event.starts_at_tz.unwrap(),
                created_by: current_user.id,
                kind: if let Some(EventStatus::Cancelled) = patch.status {
                    EventExceptionKind::Cancelled
                } else {
                    EventExceptionKind::Modified
                },
                title: patch.title,
                description: patch.description,
                is_all_day: patch.is_all_day,
                starts_at: patch.starts_at.map(|dt| dt.to_datetime_tz()),
                starts_at_tz: patch.starts_at.map(|dt| dt.timezone),
                ends_at: patch.ends_at.map(|dt| dt.to_datetime_tz()),
                ends_at_tz: patch.ends_at.map(|dt| dt.timezone),
            };

            new_exception.insert(&mut conn)?
        };

        let (invitees, invitees_truncated) =
            super::get_invitees_for_event(&settings, &mut conn, event_id, query.invitees_max)?;

        let users = GetUserProfilesBatched::new()
            .add(&event)
            .add(&exception)
            .fetch(&settings, &mut conn)?;

        let room = EventRoomInfo::from_room(&settings, room, sip_config);

        let can_edit = can_edit(&event, &current_user);

        let event_instance = create_event_instance(
            &users,
            event,
            invite
                .map(|inv| inv.status)
                .unwrap_or(EventInviteStatus::Accepted),
            is_favorite,
            Some(exception),
            room,
            instance_id,
            invitees,
            invitees_truncated,
            can_edit,
        )?;

        Ok(event_instance)
    })
    .await??;

    let event_instance = EventInstance {
        invitees: enrich_invitees_from_keycloak(
            &kc_admin_client,
            &current_tenant,
            event_instance.invitees,
        )
        .await,
        ..event_instance
    };

    Ok(Either::Left(ApiResponse::new(event_instance)))
}

#[allow(clippy::too_many_arguments)]
fn create_event_instance(
    users: &UserProfilesBatch,
    mut event: Event,
    invite_status: EventInviteStatus,
    is_favorite: bool,
    exception: Option<EventException>,
    room: EventRoomInfo,
    instance_id: InstanceId,
    invitees: Vec<EventInvitee>,
    invitees_truncated: bool,
    can_edit: bool,
) -> database::Result<EventInstance> {
    let mut status = EventStatus::Ok;

    let mut instance_starts_at = instance_id.0;
    let mut instance_starts_at_tz = event.starts_at_tz.unwrap();

    let mut instance_ends_at =
        instance_id.0 + chrono::Duration::seconds(event.duration_secs.unwrap() as i64);
    let mut instance_ends_at_tz = event.ends_at_tz.unwrap();

    if let Some(exception) = exception {
        event.updated_by = exception.created_by;
        event.updated_at = exception.created_at;

        patch(&mut event.title, exception.title);
        patch(&mut event.description, exception.description);

        match exception.kind {
            EventExceptionKind::Modified => {
                // Do nothing for now
            }
            EventExceptionKind::Cancelled => status = EventStatus::Cancelled,
        }

        patch(&mut instance_starts_at, exception.starts_at);
        patch(&mut instance_starts_at_tz, exception.starts_at_tz);
        patch(&mut instance_ends_at, exception.ends_at);
        patch(&mut instance_ends_at_tz, exception.ends_at_tz);
    }

    let created_by = users.get(event.created_by);
    let updated_by = users.get(event.updated_by);

    Ok(EventInstance {
        id: EventAndInstanceId(event.id, instance_id),
        recurring_event_id: event.id,
        instance_id,
        created_by,
        created_at: event.created_at,
        updated_by,
        updated_at: event.updated_at,
        title: event.title,
        description: event.description,
        room,
        invitees_truncated,
        invitees,
        is_all_day: event.is_all_day.unwrap(),
        starts_at: DateTimeTz {
            datetime: instance_starts_at,
            timezone: instance_starts_at_tz,
        },
        ends_at: DateTimeTz {
            datetime: instance_ends_at,
            timezone: instance_ends_at_tz,
        },
        type_: EventType::Instance,
        status,
        invite_status,
        is_favorite,
        can_edit,
    })
}

fn patch<T>(dst: &mut T, value: Option<T>) {
    if let Some(value) = value {
        *dst = value;
    }
}

fn build_rruleset(event: &Event) -> Result<RRuleSet, ApiError> {
    // TODO add recurring check into SQL query?
    if !event.is_recurring.unwrap_or_default() {
        return Err(ApiError::not_found());
    }

    // TODO add more information to internal errors here
    let recurrence_pattern = event
        .recurrence_pattern
        .as_ref()
        .ok_or_else(ApiError::internal)?;
    let starts_at = event.starts_at.ok_or_else(ApiError::internal)?;
    let starts_at_tz = event.starts_at_tz.ok_or_else(ApiError::internal)?.0;

    let starts_at = starts_at
        .with_timezone(&starts_at_tz)
        .naive_local()
        .format(LOCAL_DT_FORMAT);

    let rruleset = format!("DTSTART;TZID={starts_at_tz}:{starts_at};\n{recurrence_pattern}");
    let rruleset: RRuleSet = rruleset.parse().map_err(|e| {
        log::error!("failed to parse rrule from db {}", e);
        ApiError::internal()
    })?;

    Ok(rruleset)
}

fn verify_recurrence_date(
    event: &Event,
    requested_dt: DateTime<Utc>,
) -> Result<RRuleSet, ApiError> {
    let rruleset = build_rruleset(event)?;

    let requested_dt = requested_dt.with_timezone(&event.starts_at_tz.unwrap().0);

    // Find date in recurrence, if it does not exist this will return a 404
    // And if it finds it it will break the loop
    let found = rruleset
        .into_iter()
        .take(36525)
        .take_while(|x| x <= &requested_dt)
        .any(|x| x == requested_dt);

    if found {
        Ok(rruleset)
    } else {
        Err(ApiError::not_found())
    }
}

#[cfg(test)]
mod tests {
    use crate::api::v1::events::EventInviteeProfile;

    use super::*;
    use db_storage::events::TimeZone;
    use db_storage::users::UserId;
    use std::time::SystemTime;
    use test_util::assert_eq_json;
    use types::core::RoomId;
    use uuid::Uuid;

    #[test]
    fn event_instance_serialize() {
        let unix_epoch: DateTime<Utc> = SystemTime::UNIX_EPOCH.into();
        let instance_id = InstanceId(unix_epoch);
        let event_id = EventId::from(Uuid::nil());
        let user_profile = PublicUserProfile {
            id: UserId::from(Uuid::nil()),
            email: "test@example.org".into(),
            title: "".into(),
            firstname: "Test".into(),
            lastname: "Test".into(),
            display_name: "Tester".into(),
            avatar_url: "https://example.org/avatar".into(),
        };

        let instance = EventInstance {
            id: EventAndInstanceId(event_id, instance_id),
            recurring_event_id: event_id,
            instance_id,
            created_by: user_profile.clone(),
            created_at: unix_epoch,
            updated_by: user_profile.clone(),
            updated_at: unix_epoch,
            title: "Instance title".into(),
            description: "Instance description".into(),
            room: EventRoomInfo {
                id: RoomId::from(Uuid::nil()),
                password: None,
                waiting_room: false,
                sip_tel: None,
                sip_uri: None,
                sip_id: None,
                sip_password: None,
            },
            invitees_truncated: false,
            invitees: vec![EventInvitee {
                profile: EventInviteeProfile::Registered(user_profile),
                status: EventInviteStatus::Accepted,
            }],
            is_all_day: false,
            starts_at: DateTimeTz {
                datetime: unix_epoch,
                timezone: TimeZone(Tz::Europe__Berlin),
            },
            ends_at: DateTimeTz {
                datetime: unix_epoch,
                timezone: TimeZone(Tz::Europe__Berlin),
            },
            type_: EventType::Instance,
            status: EventStatus::Ok,
            invite_status: EventInviteStatus::Accepted,
            is_favorite: false,
            can_edit: false,
        };

        assert_eq_json!(
            instance,
            {
                "id": "00000000-0000-0000-0000-000000000000_19700101T000000Z",
                "recurring_event_id": "00000000-0000-0000-0000-000000000000",
                "instance_id": "19700101T000000Z",
                "created_by": {
                    "id": "00000000-0000-0000-0000-000000000000",
                    "email": "test@example.org",
                    "title": "",
                    "firstname": "Test",
                    "lastname": "Test",
                    "display_name": "Tester",
                    "avatar_url": "https://example.org/avatar"
                },
                "created_at": "1970-01-01T00:00:00Z",
                "updated_by": {
                    "id": "00000000-0000-0000-0000-000000000000",
                    "email": "test@example.org",
                    "title": "",
                    "firstname": "Test",
                    "lastname": "Test",
                    "display_name": "Tester",
                    "avatar_url": "https://example.org/avatar"
                },
                "updated_at": "1970-01-01T00:00:00Z",
                "title": "Instance title",
                "description": "Instance description",
                "room": {
                    "id": "00000000-0000-0000-0000-000000000000",
                    "waiting_room": false
                },
                "invitees_truncated": false,
                "invitees": [
                    {
                        "profile": {
                            "kind": "registered",
                            "id": "00000000-0000-0000-0000-000000000000",
                            "email": "test@example.org",
                            "title": "",
                            "firstname": "Test",
                            "lastname": "Test",
                            "display_name": "Tester",
                            "avatar_url": "https://example.org/avatar"
                        },
                        "status": "accepted"
                    }
                ],
                "is_all_day": false,
                "starts_at": {
                    "datetime": "1970-01-01T00:00:00Z",
                    "timezone": "Europe/Berlin"
                },
                "ends_at": {
                    "datetime": "1970-01-01T00:00:00Z",
                    "timezone": "Europe/Berlin"
                },
                "type": "instance",
                "status": "ok",
                "invite_status": "accepted",
                "is_favorite": false,
                "can_edit": false,
            }
        );
    }
}
