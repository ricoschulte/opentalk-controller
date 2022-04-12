use super::cursor::Cursor;
use super::request::default_pagination_per_page;
use super::response::NoContent;
use super::users::PublicUserProfile;
use super::{ApiResponse, DefaultApiError, DefaultApiResult, PagePaginationQuery};
use crate::api::v1::rooms::RoomsPoliciesBuilderExt;
use crate::api::v1::util::GetUserProfilesBatched;
use crate::settings::SharedSettingsActix;
use actix_web::web::{Data, Json, Path, Query, ReqData};
use actix_web::{delete, get, patch, post};
use chrono::{DateTime, Datelike, NaiveTime, TimeZone as _, Utc};
use chrono_tz::Tz;
use controller_shared::settings::Settings;
use database::{Db, DbConnection};
use db_storage::events::{
    Event, EventException, EventExceptionKind, EventId, EventInvite, EventInviteStatus, NewEvent,
    TimeZone, UpdateEvent,
};
use db_storage::rooms::{NewRoom, Room, RoomId};
use db_storage::sip_configs::{NewSipConfig, SipConfig};
use db_storage::users::User;
use kustos::policies_builder::{GrantingAccess, PoliciesBuilder};
use kustos::prelude::{AccessMethod, IsSubject};
use kustos::{Authz, Resource};
use rrule::{Frequency, RRuleSet};
use serde::de::Visitor;
use serde::{Deserialize, Serialize};
use validator::{Validate, ValidationError};

pub mod favorites;
pub mod instances;
pub mod invites;

const LOCAL_DT_FORMAT: &str = "%Y%m%dT%H%M%S";
const UTC_DT_FORMAT: &str = "%Y%m%dT%H%M%SZ";

/// Opaque id of an EventInstance or EventException resource. Should only be used to sort/index the related resource.
#[derive(Debug, Copy, Clone)]
pub struct EventAndInstanceId(EventId, InstanceId);

impl Serialize for EventAndInstanceId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        format!("{}_{}", self.0, (self.1).0.format(UTC_DT_FORMAT)).serialize(serializer)
    }
}

/// ID of an EventInstance
///
/// Is created from the starts_at datetime of the original recurrence (original meaning that exceptions don't change
/// the instance id).
#[derive(Debug, Copy, Clone)]
pub struct InstanceId(DateTime<Utc>);

impl Serialize for InstanceId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0
            .format(UTC_DT_FORMAT)
            .to_string()
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for InstanceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(InstanceIdVisitor)
    }
}

struct InstanceIdVisitor;
impl<'de> Visitor<'de> for InstanceIdVisitor {
    type Value = InstanceId;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "timestamp in '{}' format", UTC_DT_FORMAT)
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Utc.datetime_from_str(v, UTC_DT_FORMAT)
            .map(InstanceId)
            .map_err(|_| serde::de::Error::invalid_value(serde::de::Unexpected::Str(v), &self))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DateTimeTz {
    /// UTC datetime
    pub datetime: DateTime<Utc>,
    /// Timezone in which the datetime was created in
    pub timezone: TimeZone,
}

impl DateTimeTz {
    /// Create a [`DateTimeTz`] from the database results
    ///
    /// Returns None if any of them are none.
    ///
    /// Only used to exceptions. To get the correct starts_at/ends_at [`DateTimeTz`] values
    /// [`DateTimeTz::starts_at_of`] and [`DateTimeTz::ends_at_of`] is used
    fn maybe_from_db(utc_dt: Option<DateTime<Utc>>, tz: Option<TimeZone>) -> Option<Self> {
        if let (Some(utc_dt), Some(tz)) = (utc_dt, tz) {
            Some(Self {
                datetime: utc_dt,
                timezone: tz,
            })
        } else {
            None
        }
    }

    /// Creates the `starts_at` DateTimeTz from an event
    fn starts_at_of(event: &Event) -> Option<Self> {
        if let (Some(dt), Some(tz)) = (event.starts_at, event.starts_at_tz) {
            Some(Self {
                datetime: dt,
                timezone: tz,
            })
        } else {
            None
        }
    }

    /// Creates the `ends_at` DateTimeTz from an event
    fn ends_at_of(event: &Event) -> Option<Self> {
        if event.is_recurring.unwrap_or_default() {
            // Recurring events have the last occurrence of the recurrence saved in the ends_at fields
            // So we get the starts_at_dt and add the duration_secs field to it
            if let (Some(starts_at_dt), Some(dur), Some(tz)) =
                (event.starts_at, event.duration_secs, event.ends_at_tz)
            {
                Some(Self {
                    datetime: starts_at_dt + chrono::Duration::seconds(i64::from(dur)),
                    timezone: tz,
                })
            } else {
                None
            }
        } else if let (Some(dt), Some(tz)) = (event.starts_at, event.starts_at_tz) {
            // Non recurring events just directly use the ends_at field from the db
            Some(Self {
                datetime: dt,
                timezone: tz,
            })
        } else {
            None
        }
    }

    /// Combine the inner UTC time with the inner timezone
    fn to_datetime_tz(self) -> DateTime<Tz> {
        self.datetime.with_timezone(&self.timezone.0)
    }
}

/// Event Resource representation
///
/// Returned from `GET /events/` and `GET /events/{event_id}`
#[derive(Debug, Serialize)]
pub struct EventResource {
    /// ID of the event
    pub id: EventId,

    /// Public user profile of the user which created the event
    pub created_by: PublicUserProfile,

    /// Timestamp of the event creation
    pub created_at: DateTime<Utc>,

    /// Public user profile of the user which last updated the event
    pub updated_by: PublicUserProfile,

    /// Timestamp of the last update
    pub updated_at: DateTime<Utc>,

    /// Title of the event
    ///
    /// For display purposes
    pub title: String,

    /// Description of the event
    ///
    /// For display purposes
    pub description: String,

    /// All information about the room the event takes place in
    pub room: EventRoomInfo,

    /// Flag which indicates if `invitees` contains all invites as far as known to the application
    /// May also be true if there are no invitees but no invitees were requested
    pub invitees_truncated: bool,

    /// List of event invitees and their invite status. Might not be complete, see `invite_truncated`
    pub invitees: Vec<EventInvitee>,

    /// Is the event time independent?
    ///
    /// Time independent events are not bound to any time but instead are constantly available to join
    pub is_time_independent: bool,

    /// Is the event an all day event
    ///
    /// All-day events have no start/end time, they last the entire day(s)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_all_day: Option<bool>,

    /// Start time of the event.
    ///
    /// Omitted if `is_time_independent` is true
    ///
    /// For events of type `recurring` the datetime contains the time of the first instance.
    /// The datetimes of subsequent recurrences are computed using the datetime of the first instance and its timezone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<DateTimeTz>,

    /// End time of the event.
    ///
    /// Omitted if `is_time_independent` is true
    ///
    /// For events of type `recurring` the datetime contains the time of the first instance.
    /// The datetimes of subsequent recurrences are computed using the datetime of the first instance and its timezone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ends_at: Option<DateTimeTz>,

    /// Recurrence pattern(s) for recurring events
    ///
    /// May contain RRULE, EXRULE, RDATE and EXDATE strings
    ///
    /// Requires `type` to be `recurring`
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub recurrence_pattern: Vec<String>,

    /// Type of event
    ///
    /// Time independent events or events without recurrence are `single` while recurring events are `recurring`
    #[serde(rename = "type")]
    pub type_: EventType,

    /// Status of the event. `ok` by default but may be `cancelled`
    pub status: EventStatus,

    /// The invite status of the current user for this event
    pub invite_status: EventInviteStatus,

    /// Is this event in the current user's favorite list?
    pub is_favorite: bool,
}

/// Event exception resource
///
/// Overrides event properties for a event recurrence. May only exist for events of type `recurring`.
#[derive(Debug, Serialize)]
pub struct EventExceptionResource {
    /// Opaque ID of the exception
    pub id: EventAndInstanceId,

    /// ID of the event  the exception belongs to
    pub recurring_event_id: EventId,

    /// ID of the instance the exception overrides
    pub instance_id: InstanceId,

    /// Public user profile of the user which created the exception
    pub created_by: PublicUserProfile,

    /// Timestamp of the exceptions creation
    pub created_at: DateTime<Utc>,

    /// Public user profile of the user which last updated the exception
    pub updated_by: PublicUserProfile,

    /// Timestamp of the exceptions last update
    pub updated_at: DateTime<Utc>,

    /// Override the title of the instance
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Override the description of the instance
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Override the `is_all_day` property of the instance
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_all_day: Option<bool>,

    /// Override the `starts_at` time of the instance
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<DateTimeTz>,

    /// Override the `ends_at` time of the instance
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ends_at: Option<DateTimeTz>,

    /// The `starts_at` of the instance this exception modifies. Used to match the exception the instance
    pub original_starts_at: DateTimeTz,

    /// Must always be `exception`
    #[serde(rename = "type")]
    pub type_: EventType,

    /// Override the status of the event instance
    ///
    /// This can be used to cancel a occurrence of an event
    pub status: EventStatus,
}

impl EventExceptionResource {
    pub fn from_db(exception: EventException, created_by: PublicUserProfile) -> Self {
        Self {
            id: EventAndInstanceId(exception.event_id, InstanceId(exception.exception_date)),
            recurring_event_id: exception.event_id,
            instance_id: InstanceId(exception.exception_date),
            created_by: created_by.clone(),
            created_at: exception.created_at,
            updated_by: created_by,
            updated_at: exception.created_at,
            title: exception.title,
            description: exception.description,
            is_all_day: exception.is_all_day,
            starts_at: DateTimeTz::maybe_from_db(exception.starts_at, exception.starts_at_tz),
            ends_at: DateTimeTz::maybe_from_db(exception.ends_at, exception.ends_at_tz),
            original_starts_at: DateTimeTz {
                datetime: exception.exception_date,
                timezone: exception.exception_date_tz,
            },
            type_: EventType::Exception,
            status: match exception.kind {
                EventExceptionKind::Modified => EventStatus::Ok,
                EventExceptionKind::Cancelled => EventStatus::Cancelled,
            },
        }
    }
}

/// Invitee to an event
///
///  Contains user profile and invitee status
#[derive(Debug, Clone, Serialize)]
pub struct EventInvitee {
    pub profile: PublicUserProfile,
    pub status: EventInviteStatus,
}

/// Type of event resource.
///
/// Is used as type discriminator in field `type`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Single,
    Recurring,
    Instance,
    Exception,
}

/// Status of an event
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    /// Default status, event is ok
    Ok,

    /// Event (or event instance) was cancelled
    Cancelled,
}

/// All information about a room in which an event takes place
#[derive(Debug, Clone, Serialize)]
pub struct EventRoomInfo {
    /// ID of the room
    pub id: RoomId,

    /// Password of the room
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,

    /// SIP Call-In phone number which must be used to reach the room
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sip_tel: Option<String>,

    /// SIP Call-In sip uri
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sip_uri: Option<String>,

    /// SIP ID which must transmitted via DTMF (number field on the phone) to identify this room
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sip_id: Option<String>,

    /// SIP password which must be transmitted via DTMF (number field on the phone) after entering the `sip_id`
    /// to enter the room
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sip_password: Option<String>,
}

impl EventRoomInfo {
    fn from_room(settings: &Settings, room: Room, sip_config: Option<SipConfig>) -> Self {
        let sip_tel = if sip_config.is_some() {
            settings.call_in.as_ref().map(|c| c.tel.clone())
        } else {
            None
        };

        let (sip_id, sip_password) = if let Some(sip_config) = sip_config {
            (
                Some(sip_config.sip_id.into_inner().into_inner()),
                Some(sip_config.password.into_inner().into_inner()),
            )
        } else {
            (None, None)
        };
        Self {
            id: room.id,
            password: if room.password.is_empty() {
                None
            } else {
                Some(room.password)
            },
            sip_tel,
            sip_uri: None, // TODO SIP URI support
            sip_id,
            sip_password,
        }
    }
}

/// Body of the the `POST /events` endpoint
#[derive(Debug, Deserialize, Validate)]
pub struct PostEventsBody {
    /// Title of the event
    #[validate(length(max = 255))]
    pub title: String,

    /// Description of the event
    #[validate(length(max = 4096))]
    pub description: String,

    /// Optional password for the room related to the event
    #[validate(length(min = 1, max = 255))]
    pub password: Option<String>,

    /// Should the created event be time independent?
    ///
    /// If true, all following fields must be null
    /// If false, requires `is_all_day`, `starts_at`, `ends_at`
    pub is_time_independent: bool,

    /// Should the event be all-day?
    ///
    /// If true, requires `starts_at.datetime` and `ends_at.datetime` to have a 00:00 time part
    pub is_all_day: Option<bool>,

    /// Start time of the event
    ///
    /// For recurring events these must contains the datetime of the first instance
    pub starts_at: Option<DateTimeTz>,

    /// End time of the event
    ///
    /// For recurring events these must contains the datetime of the first instance
    pub ends_at: Option<DateTimeTz>,

    /// List of recurrence patterns
    ///
    /// If the list if non-empty the created event will be of type `recurring`
    ///
    /// For more infos see the documentation of [`EventResource`]
    #[validate(custom = "validate_recurrence_pattern")]
    #[serde(default)]
    pub recurrence_pattern: Vec<String>,
}

fn validate_recurrence_pattern(pattern: &[String]) -> Result<(), ValidationError> {
    if pattern.len() > 4 {
        return Err(ValidationError::new("too_many_recurrence_patterns"));
    }

    if pattern.iter().any(|p| p.len() > 1024) {
        return Err(ValidationError::new("recurrence_pattern_too_large"));
    }

    Ok(())
}

/// API Endpoint `POST /events`
#[post("/events")]
pub async fn new_event(
    settings: SharedSettingsActix,
    db: Data<Db>,
    authz: Data<Authz>,
    current_user: ReqData<User>,
    new_event: Json<PostEventsBody>,
) -> DefaultApiResult<EventResource> {
    let settings = settings.load_full();
    let current_user = current_user.into_inner();
    let new_event = new_event.into_inner();

    if let Err(_e) = new_event.validate() {
        return Err(DefaultApiError::ValidationFailed);
    }

    let event_resource = crate::block(move || {
        let conn = db.get_conn()?;

        // simplify logic by splitting the event creation
        // into two paths: time independent and time dependent
        match new_event {
            PostEventsBody {
                title,
                description,
                password,
                is_time_independent: true,
                is_all_day: None,
                starts_at: None,
                ends_at: None,
                recurrence_pattern,
            } if recurrence_pattern.is_empty() => {
                create_time_independent_event(
                    &settings,
                    &conn,
                    current_user,
                    title,
                    description,
                    password,
                )
            }
            PostEventsBody {
                title,
                description,
                password,
                is_time_independent: false,
                is_all_day: Some(is_all_day),
                starts_at: Some(starts_at),
                ends_at: Some(ends_at),
                recurrence_pattern,
            } => {
                create_time_dependent_event(
                    &settings,
                    &conn,
                    current_user,
                    title,
                    description,
                    password,
                    is_all_day,
                    starts_at,
                    ends_at,
                    recurrence_pattern,
                )
            }
            new_event => {
                let msg = if new_event.is_time_independent {
                    "time independent events must not have is_all_day, starts_at, ends_at or recurrence_pattern set"
                } else {
                     "time dependent events must have title, description, is_all_day, starts_at and ends_at set"
                };

                Err(DefaultApiError::BadRequest(msg.into()))
            }
        }
    })
    .await??;

    let policies = PoliciesBuilder::new()
        .grant_user_access(event_resource.created_by.id)
        .event_read_access(event_resource.id)
        .event_write_access(event_resource.id)
        .room_read_access(event_resource.room.id)
        .room_write_access(event_resource.room.id)
        .finish();

    if let Err(e) = authz.add_policies(policies).await {
        log::error!("Failed to add RBAC policies: {}", e);
        return Err(DefaultApiError::Internal);
    }

    Ok(ApiResponse::new(event_resource))
}

/// Part of `POST /events` endpoint
fn create_time_independent_event(
    settings: &Settings,
    conn: &DbConnection,
    current_user: User,
    title: String,
    description: String,
    password: Option<String>,
) -> Result<EventResource, DefaultApiError> {
    let room = NewRoom {
        created_by: current_user.id,
        password: password.unwrap_or_default(),
        wait_for_moderator: false,
        listen_only: false,
    }
    .insert(conn)?;

    let sip_config = NewSipConfig::new(room.id, false).insert(conn)?;

    let event = NewEvent {
        title,
        description,
        room: room.id,
        created_by: current_user.id,
        updated_by: current_user.id,
        is_time_independent: true,
        is_all_day: None,
        starts_at: None,
        starts_at_tz: None,
        ends_at: None,
        ends_at_tz: None,
        duration_secs: None,
        is_recurring: None,
        recurrence_pattern: None,
    }
    .insert(conn)?;

    Ok(EventResource {
        id: event.id,
        title: event.title,
        description: event.description,
        room: EventRoomInfo::from_room(settings, room, Some(sip_config)),
        invitees_truncated: false,
        invitees: vec![],
        created_by: PublicUserProfile::from_db(settings, current_user.clone()),
        created_at: event.created_at,
        updated_by: PublicUserProfile::from_db(settings, current_user),
        updated_at: event.updated_at,
        is_time_independent: true,
        is_all_day: None,
        starts_at: None,
        ends_at: None,
        recurrence_pattern: vec![],
        type_: EventType::Single,
        status: EventStatus::Ok,
        invite_status: EventInviteStatus::Accepted,
        is_favorite: false,
    })
}

/// Part of `POST /events` endpoint
#[allow(clippy::too_many_arguments)]
fn create_time_dependent_event(
    settings: &Settings,
    conn: &DbConnection,
    current_user: User,
    title: String,
    description: String,
    password: Option<String>,
    is_all_day: bool,
    starts_at: DateTimeTz,
    ends_at: DateTimeTz,
    recurrence_pattern: Vec<String>,
) -> Result<EventResource, DefaultApiError> {
    let recurrence_pattern = recurrence_array_to_string(recurrence_pattern);

    let (duration_secs, ends_at_dt, ends_at_tz) =
        parse_event_dt_params(is_all_day, starts_at, ends_at, &recurrence_pattern)?;

    let room = NewRoom {
        created_by: current_user.id,
        password: password.unwrap_or_default(),
        wait_for_moderator: false,
        listen_only: false,
    }
    .insert(conn)?;

    let sip_config = NewSipConfig::new(room.id, false).insert(conn)?;

    let event = NewEvent {
        title,
        description,
        room: room.id,
        created_by: current_user.id,
        updated_by: current_user.id,
        is_time_independent: false,
        is_all_day: Some(is_all_day),
        starts_at: Some(starts_at.to_datetime_tz()),
        starts_at_tz: Some(starts_at.timezone),
        ends_at: Some(ends_at_dt),
        ends_at_tz: Some(ends_at_tz),
        duration_secs,
        is_recurring: Some(recurrence_pattern.is_some()),
        recurrence_pattern,
    }
    .insert(conn)?;

    Ok(EventResource {
        id: event.id,
        title: event.title,
        description: event.description,
        room: EventRoomInfo::from_room(settings, room, Some(sip_config)),
        invitees_truncated: false,
        invitees: vec![],
        created_by: PublicUserProfile::from_db(settings, current_user.clone()),
        created_at: event.created_at,
        updated_by: PublicUserProfile::from_db(settings, current_user),
        updated_at: event.updated_at,
        is_time_independent: false,
        is_all_day: event.is_all_day,
        starts_at: Some(starts_at),
        ends_at: Some(ends_at),
        recurrence_pattern: recurrence_string_to_array(event.recurrence_pattern),
        type_: if event.is_recurring.unwrap_or_default() {
            EventType::Recurring
        } else {
            EventType::Single
        },
        status: EventStatus::Ok,
        invite_status: EventInviteStatus::Accepted,
        is_favorite: false,
    })
}

/// Path query parameters of the `GET /events` endpoint
///
/// Allows for customization in the search for events
#[derive(Debug, Deserialize)]
pub struct GetEventsQuery {
    /// Optional minimum time in which the event happens
    time_min: Option<DateTime<Utc>>,

    /// Optional maximum time in which the event happens
    time_max: Option<DateTime<Utc>>,

    /// Maximum number of invitees to return inside the event resource
    ///
    /// Default: 0
    #[serde(default)]
    invitees_max: u32,

    /// Return only favorite events
    #[serde(default)]
    favorites: bool,

    /// How many events to return per page
    per_page: Option<i64>,

    /// Cursor token to get the next page of events
    ///
    /// Returned by the endpoint if the maximum number of events per page has been hit
    after: Option<Cursor<GetEventsCursorData>>,
}

/// Data stored inside the `GET /events` query cursor
#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
struct GetEventsCursorData {
    /// Last event in the list
    event_id: EventId,

    /// last event created at
    event_created_at: DateTime<Utc>,

    /// Last event starts_at
    event_starts_at: Option<DateTime<Utc>>,
}

/// Return type of the `GET /events` endpoint
#[derive(Serialize)]
#[serde(untagged)]
pub enum EventOrException {
    Event(EventResource),
    Exception(EventExceptionResource),
}

/// API Endpoint `GET /events`
///
/// Returns a paginated list of events and their exceptions inside the given time range
///
/// See documentation of [`GetEventsQuery`] for all query options
#[get("/events")]
pub async fn get_events(
    settings: SharedSettingsActix,
    db: Data<Db>,
    current_user: ReqData<User>,
    query: Query<GetEventsQuery>,
) -> DefaultApiResult<Vec<EventOrException>> {
    let settings = settings.load_full();
    let current_user = current_user.into_inner();
    let query = query.into_inner();

    crate::block(move || {
        let per_page = query
            .per_page
            .unwrap_or_else(default_pagination_per_page)
            .max(1)
            .min(100);

        let mut users = GetUserProfilesBatched::new();

        let get_events_cursor = query
            .after
            .map(|cursor| db_storage::events::GetEventsCursor {
                from_id: cursor.event_id,
                from_created_at: cursor.event_created_at,
                from_starts_at: cursor.event_starts_at,
            });

        let conn = db.get_conn()?;

        let events = Event::get_all_for_user_paginated(
            &conn,
            current_user.id,
            query.favorites,
            query.time_min,
            query.time_max,
            get_events_cursor,
            per_page,
        )?;

        for (event, _, _, _, exceptions, _) in &events {
            users.add(event);
            users.add(exceptions);
        }

        let users = users.fetch(&settings, &conn)?;

        let event_refs: Vec<&Event> = events.iter().map(|(event, ..)| event).collect();

        // Build list of event-invite with user, grouped by events
        let invites_with_users_grouped_by_event = if query.invitees_max == 0 {
            // Do not query event invites if invitees_max is zero, instead create dummy value
            (0..events.len()).map(|_| Vec::new()).collect()
        } else {
            EventInvite::get_for_events(&conn, &event_refs)?
        };

        let mut event_resources = vec![];

        let mut ret_cursor_data = None;

        for ((event, invite, room, sip_config, exceptions, is_favorite), mut invites_with_user) in
            events.into_iter().zip(invites_with_users_grouped_by_event)
        {
            ret_cursor_data = Some(GetEventsCursorData {
                event_id: event.id,
                event_created_at: event.created_at,
                event_starts_at: event.starts_at,
            });

            let created_by = users.get(event.created_by);
            let updated_by = users.get(event.updated_by);

            let invite_status = invite
                .map(|invite| invite.status)
                .unwrap_or(EventInviteStatus::Accepted);

            let invitees_truncated =
                query.invitees_max == 0 || invites_with_user.len() > query.invitees_max as usize;

            invites_with_user.truncate(query.invitees_max as usize);

            let invitees = invites_with_user
                .into_iter()
                .map(|(invite, user)| EventInvitee {
                    profile: PublicUserProfile::from_db(&settings, user),
                    status: invite.status,
                })
                .collect();

            let starts_at = DateTimeTz::starts_at_of(&event);
            let ends_at = DateTimeTz::ends_at_of(&event);

            event_resources.push(EventOrException::Event(EventResource {
                id: event.id,
                created_by,
                created_at: event.created_at,
                updated_by,
                updated_at: event.updated_at,
                title: event.title,
                description: event.description,
                room: EventRoomInfo::from_room(&settings, room, sip_config),
                invitees_truncated,
                invitees,
                is_time_independent: event.is_time_independent,
                is_all_day: event.is_all_day,
                starts_at,
                ends_at,
                recurrence_pattern: recurrence_string_to_array(event.recurrence_pattern),
                type_: if event.is_recurring.unwrap_or_default() {
                    EventType::Recurring
                } else {
                    EventType::Single
                },
                status: EventStatus::Ok,
                invite_status,
                is_favorite,
            }));

            for exception in exceptions {
                let created_by = users.get(exception.created_by);

                event_resources.push(EventOrException::Exception(
                    EventExceptionResource::from_db(exception, created_by),
                ));
            }
        }

        Ok(ApiResponse::new(event_resources)
            .with_cursor_pagination(None, ret_cursor_data.map(|c| Cursor(c).to_base64())))
    })
    .await?
}

/// Path query parameters for the `GET /events/{event_id}` endpoint
#[derive(Debug, Deserialize)]
pub struct GetEventQuery {
    /// Maximum number of invitees to return inside the event resource
    ///
    /// Default: 0
    #[serde(default)]
    invitees_max: i64,
}

/// API Endpoint `GET /events/{event_id}` endpoint
///
/// Returns the event resource for the given id
#[get("/events/{event_id}")]
pub async fn get_event(
    settings: SharedSettingsActix,
    db: Data<Db>,
    current_user: ReqData<User>,
    event_id: Path<EventId>,
    query: Query<GetEventQuery>,
) -> DefaultApiResult<EventResource> {
    let settings = settings.load_full();
    let event_id = event_id.into_inner();
    let query = query.into_inner();

    crate::block(move || {
        let conn = db.get_conn()?;

        let (event, invite, room, sip_config, is_favorite) =
            Event::get_with_invite_and_room(&conn, current_user.id, event_id)?;
        let (invitees, invitees_truncated) =
            get_invitees_for_event(&settings, &conn, event_id, query.invitees_max)?;

        let users = GetUserProfilesBatched::new()
            .add(&event)
            .fetch(&settings, &conn)?;

        let starts_at = DateTimeTz::starts_at_of(&event);
        let ends_at = DateTimeTz::ends_at_of(&event);

        let event_resource = EventResource {
            id: event.id,
            title: event.title,
            description: event.description,
            room: EventRoomInfo::from_room(&settings, room, sip_config),
            invitees_truncated,
            invitees,
            created_by: users.get(event.created_by),
            created_at: event.created_at,
            updated_by: users.get(event.updated_by),
            updated_at: event.updated_at,
            is_time_independent: event.is_time_independent,
            is_all_day: event.is_all_day,
            starts_at,
            ends_at,
            recurrence_pattern: recurrence_string_to_array(event.recurrence_pattern),
            type_: if event.is_recurring.unwrap_or_default() {
                EventType::Recurring
            } else {
                EventType::Single
            },
            status: EventStatus::Ok,
            invite_status: invite
                .map(|inv| inv.status)
                .unwrap_or(EventInviteStatus::Accepted),
            is_favorite,
        };

        Ok(ApiResponse::new(event_resource))
    })
    .await?
}

/// Path query parameters for the `PATCH /events/{event_id}` endpoint
#[derive(Debug, Deserialize)]
pub struct PatchEventQuery {
    /// Maximum number of invitees to include inside the event
    #[serde(default)]
    invitees_max: i64,
}

/// Body for the `PATCH /events/{event_id}` endpoint
#[derive(Deserialize, Validate)]
pub struct PatchEventBody {
    /// Patch the title of th event
    #[validate(length(max = 255))]
    title: Option<String>,

    /// Patch the description of the event
    #[validate(length(max = 4096))]
    description: Option<String>,

    /// Patch the time independence of the event
    ///
    /// If it changes the independence from true false this body has to have
    /// `is_all_day`, `starts_at` and `ends_at` set
    ///
    /// See documentation of [`PostEventsBody`] for more info
    is_time_independent: Option<bool>,

    /// Patch if the event is an all-day event
    ///
    /// If it changes the value from false to true this request must ensure
    /// that the `starts_at.datetime` and `ends_at.datetime` have a 00:00 time part.
    ///
    /// See documentation of [`PostEventsBody`] for more info
    is_all_day: Option<bool>,

    starts_at: Option<DateTimeTz>,
    ends_at: Option<DateTimeTz>,

    /// Patch the events recurrence patterns
    ///
    /// If this list is non empty it override the events current one
    #[validate(custom = "validate_recurrence_pattern")]
    #[serde(default)]
    recurrence_pattern: Vec<String>,
}

/// API Endpoint `PATCH /events/{event_id}`
///
/// See documentation of [`PatchEventBody`] for more infos
///
/// Patches which modify the event in a way that would invalidate existing
/// exceptions (e.g. by changing the recurrence rule or time dependence)
/// will have all exceptions deleted
#[patch("/events/{event_id}")]
pub async fn patch_event(
    settings: SharedSettingsActix,
    db: Data<Db>,
    current_user: ReqData<User>,
    event_id: Path<EventId>,
    query: Query<PatchEventQuery>,
    patch: Json<PatchEventBody>,
) -> DefaultApiResult<EventResource> {
    let settings = settings.load_full();
    let current_user = current_user.into_inner();
    let event_id = event_id.into_inner();
    let query = query.into_inner();
    let patch = patch.into_inner();

    if let Err(_e) = patch.validate() {
        return Err(DefaultApiError::ValidationFailed);
    }

    crate::block(move || {
        let conn = db.get_conn()?;

        let (event, invite, room, sip_config, is_favorite) =
            Event::get_with_invite_and_room(&conn, current_user.id, event_id)?;

        let update_event = match (event.is_time_independent, patch.is_time_independent) {
            (true, Some(false)) => {
                // The patch changes the event from an time-independent event
                // to a time dependent event
                patch_event_change_to_time_dependent(&current_user, patch)?
            }
            (true, _) | (false, Some(true)) => {
                // The patch will modify an time-independent event or
                // change an event to a time-independent event
                patch_time_independent_event(&conn, &current_user, &event, patch)?
            }
            _ => {
                // The patch modifies an time dependent event
                patch_time_dependent_event(&conn, &current_user, &event, patch)?
            }
        };

        let event = update_event.apply(&conn, event_id)?;

        let created_by = if event.created_by == current_user.id {
            current_user.clone()
        } else {
            User::get(&conn, event.created_by)?
        };

        let (invitees, invitees_truncated) =
            get_invitees_for_event(&settings, &conn, event_id, query.invitees_max)?;

        let starts_at = DateTimeTz::starts_at_of(&event);
        let ends_at = DateTimeTz::ends_at_of(&event);

        let event_resource = EventResource {
            id: event.id,
            created_by: PublicUserProfile::from_db(&settings, created_by),
            created_at: event.created_at,
            updated_by: PublicUserProfile::from_db(&settings, current_user),
            updated_at: event.updated_at,
            title: event.title,
            description: event.description,
            room: EventRoomInfo::from_room(&settings, room, sip_config),
            invitees_truncated,
            invitees,
            is_time_independent: true,
            is_all_day: event.is_all_day,
            starts_at,
            ends_at,
            recurrence_pattern: recurrence_string_to_array(event.recurrence_pattern),
            type_: if event.is_recurring.unwrap_or_default() {
                EventType::Recurring
            } else {
                EventType::Single
            },
            status: EventStatus::Ok,
            invite_status: invite
                .map(|inv| inv.status)
                .unwrap_or(EventInviteStatus::Accepted),
            is_favorite,
        };

        Ok(ApiResponse::new(event_resource))
    })
    .await?
}

/// Part of `PATCH /events/{event_id}` (see [`patch_event`])
///
/// Patch event which is time independent into a time dependent event
fn patch_event_change_to_time_dependent(
    current_user: &User,
    patch: PatchEventBody,
) -> Result<UpdateEvent, DefaultApiError> {
    if let (Some(is_all_day), Some(starts_at), Some(ends_at)) =
        (patch.is_all_day, patch.starts_at, patch.ends_at)
    {
        let recurrence_pattern = recurrence_array_to_string(patch.recurrence_pattern);

        let (duration_secs, ends_at_dt, ends_at_tz) =
            parse_event_dt_params(is_all_day, starts_at, ends_at, &recurrence_pattern)?;

        Ok(UpdateEvent {
            title: patch.title,
            description: patch.description,
            updated_by: current_user.id,
            updated_at: Utc::now(),
            is_time_independent: Some(false),
            is_all_day: Some(Some(is_all_day)),
            starts_at: Some(Some(starts_at.to_datetime_tz())),
            starts_at_tz: Some(Some(starts_at.timezone)),
            ends_at: Some(Some(ends_at_dt)),
            ends_at_tz: Some(Some(ends_at_tz)),
            duration_secs: Some(duration_secs),
            is_recurring: Some(Some(recurrence_pattern.is_some())),
            recurrence_pattern: Some(recurrence_pattern),
        })
    } else {
        Err(DefaultApiError::BadRequest(
            "is_all_day, starts_at and ends_at must be provided".into(),
        ))
    }
}

/// Part of `PATCH /events/{event_id}` (see [`patch_event`])
///
/// Patch event which is time dependent into a time independent event
fn patch_time_independent_event(
    conn: &DbConnection,
    current_user: &User,
    event: &Event,
    patch: PatchEventBody,
) -> Result<UpdateEvent, DefaultApiError> {
    if patch.is_all_day.is_some() || patch.starts_at.is_some() || patch.ends_at.is_some() {
        return Err(DefaultApiError::BadRequest(
            "is_all_day, starts_at and ends_at would be ignored in this patch".into(),
        ));
    }

    if event.is_recurring.unwrap_or_default() {
        // delete all exceptions as the time dependence has been removed
        EventException::delete_all_for_event(conn, event.id)?;
    }

    Ok(UpdateEvent {
        title: patch.title,
        description: patch.description,
        updated_by: current_user.id,
        updated_at: Utc::now(),
        is_time_independent: Some(true),
        is_all_day: Some(None),
        starts_at: Some(None),
        starts_at_tz: Some(None),
        ends_at: Some(None),
        ends_at_tz: Some(None),
        duration_secs: Some(None),
        is_recurring: Some(None),
        recurrence_pattern: Some(None),
    })
}

/// Part of `PATCH /events/{event_id}` (see [`patch_event`])
///
/// Patch fields on an time dependent event (without changing the time dependence field)
fn patch_time_dependent_event(
    conn: &DbConnection,
    current_user: &User,
    event: &Event,
    patch: PatchEventBody,
) -> Result<UpdateEvent, DefaultApiError> {
    let recurrence_pattern = recurrence_array_to_string(patch.recurrence_pattern);

    let is_all_day = patch.is_all_day.or(event.is_all_day).unwrap();
    let starts_at = patch
        .starts_at
        .or_else(|| DateTimeTz::starts_at_of(event))
        .unwrap();
    let ends_at = patch
        .ends_at
        .or_else(|| DateTimeTz::ends_at_of(event))
        .unwrap();

    let (duration_secs, ends_at_dt, ends_at_tz) =
        parse_event_dt_params(is_all_day, starts_at, ends_at, &recurrence_pattern)?;

    if event.is_recurring.unwrap_or_default() {
        // Delete all exceptions for recurring events as the patch may modify fields that influence the
        // timestamps at which instances (occurrences) are generated, making it impossible to match the
        // exceptions to instances
        EventException::delete_all_for_event(conn, event.id)?;
    }

    Ok(UpdateEvent {
        title: patch.title,
        description: patch.description,
        updated_by: current_user.id,
        updated_at: Utc::now(),
        is_time_independent: Some(false),
        is_all_day: Some(Some(is_all_day)),
        starts_at: Some(Some(starts_at.to_datetime_tz())),
        starts_at_tz: Some(Some(starts_at.timezone)),
        ends_at: Some(Some(ends_at_dt)),
        ends_at_tz: Some(Some(ends_at_tz)),
        duration_secs: Some(duration_secs),
        is_recurring: Some(Some(recurrence_pattern.is_some())),
        recurrence_pattern: Some(recurrence_pattern),
    })
}

/// API Endpoint `POST /events/{event_id}`
#[delete("/events/{event_id}")]
pub async fn delete_event(
    db: Data<Db>,
    authz: Data<Authz>,
    event_id: Path<EventId>,
) -> Result<NoContent, DefaultApiError> {
    let event_id = event_id.into_inner();

    crate::block(move || {
        let conn = db.get_conn()?;

        Event::delete_by_id(&conn, event_id)
    })
    .await??;

    let resources = vec![
        format!("/events/{event_id}"),
        format!("/events/{event_id}/instances"),
        format!("/events/{event_id}/instances/*"),
        format!("/events/{event_id}/invites"),
        format!("/events/{event_id}/invite"),
        format!("/users/me/event_favorites/{event_id}"),
    ];

    if let Err(e) = authz.remove_explicit_resources(resources).await {
        log::error!("Failed to remove  RBAC policies: {}", e);
        return Err(DefaultApiError::Internal);
    }

    Ok(NoContent)
}

#[derive(Deserialize, Validate)]
pub struct EventRescheduleBody {
    _from: DateTime<Utc>,
    _is_all_day: Option<bool>,
    _starts_at: Option<bool>,
    _ends_at: Option<bool>,
    #[validate(custom = "validate_recurrence_pattern")]
    _recurrence_pattern: Vec<String>,
}

#[post("/events/{event_id}/reschedule")]
pub async fn event_reschedule(
    _db: Data<Db>,
    _event_id: Path<EventId>,
    _body: Json<EventRescheduleBody>,
) -> actix_web::HttpResponse {
    if let Err(_e) = _body.validate() {
        // return Err(DefaultApiError::ValidationFailed);
        return actix_web::HttpResponse::NotImplemented().finish();
    }

    actix_web::HttpResponse::NotImplemented().finish()
}

fn get_invitees_for_event(
    settings: &Settings,
    conn: &DbConnection,
    event_id: EventId,
    invitees_max: i64,
) -> database::Result<(Vec<EventInvitee>, bool)> {
    if invitees_max > 0 {
        let (invites_with_user, total_invites) =
            EventInvite::get_for_event_paginated(conn, event_id, invitees_max, 1)?;

        let invitees_truncated = total_invites > invites_with_user.len() as i64;

        let invitees = invites_with_user
            .into_iter()
            .map(|(invite, user)| EventInvitee {
                profile: PublicUserProfile::from_db(settings, user),
                status: invite.status,
            })
            .collect();

        Ok((invitees, invitees_truncated))
    } else {
        Ok((vec![], true))
    }
}

fn recurrence_array_to_string(recurrence_pattern: Vec<String>) -> Option<String> {
    if recurrence_pattern.is_empty() {
        None
    } else {
        Some(recurrence_pattern.join("\n"))
    }
}

fn recurrence_string_to_array(recurrence_pattern: Option<String>) -> Vec<String> {
    recurrence_pattern
        .map(|s| s.split('\n').map(String::from).collect())
        .unwrap_or_default()
}

fn verify_exception_dt_params(
    is_all_day: bool,
    starts_at: DateTimeTz,
    ends_at: DateTimeTz,
) -> Result<(), DefaultApiError> {
    parse_event_dt_params(is_all_day, starts_at, ends_at, &None).map(|_| ())
}

/// parse the given event dt params
///
/// checks that the given params are valid to be put in the database
///
/// That means that:
/// - starts_at >= ends_at
/// - if is_all_day: starts_at & ends_at have their time part at 00:00
/// - bounded recurrence_pattern yields at least one result
///
/// returns the duration of the event if its recurring
/// and the appropriate ends_at datetime and timezone
fn parse_event_dt_params(
    is_all_day: bool,
    starts_at: DateTimeTz,
    ends_at: DateTimeTz,
    recurrence_pattern: &Option<String>,
) -> Result<(Option<i32>, DateTime<Tz>, TimeZone), DefaultApiError> {
    let starts_at_dt = starts_at.to_datetime_tz();
    let ends_at_dt = ends_at.to_datetime_tz();

    let duration_secs = (ends_at_dt - starts_at_dt).num_seconds();

    if duration_secs < 0 {
        return Err(DefaultApiError::BadRequest(
            "ends_at must not be before starts_at".into(),
        ));
    }

    if is_all_day {
        let zero = NaiveTime::from_hms(0, 0, 0);

        if starts_at.datetime.time() != zero || ends_at.datetime.time() != zero {
            return Err(DefaultApiError::BadRequest(
                "is_all_day requires starts_at/ends_at to be set at the start of the day".into(),
            ));
        }
    }

    if let Some(recurrence_pattern) = &recurrence_pattern {
        let starts_at_tz = starts_at.timezone.0;
        let starts_at_fmt = starts_at.datetime.format(LOCAL_DT_FORMAT);

        let rrule_set =
            format!("DTSTART;TZID={starts_at_tz}:{starts_at_fmt};\n{recurrence_pattern}");
        let rrule_set = match rrule_set.parse::<RRuleSet>() {
            Ok(rrule) => rrule,
            Err(e) => {
                log::warn!("failed to parse rrule {:?}", e);
                return Err(DefaultApiError::BadRequest(
                    "invalid recurrence pattern".into(),
                ));
            }
        };

        if rrule_set
            .rrule
            .iter()
            .any(|rrule| rrule.get_properties().freq > Frequency::Daily)
        {
            return Err(DefaultApiError::BadRequest(
                "frequencies below 'DAILY' are not supported".into(),
            ));
        }

        // Figure out ends_at timestamp
        // Check if all RRULEs are reasonably bounded in how far they go
        let is_bounded = rrule_set.rrule.iter().all(|rrule| {
            let properties = rrule.get_properties();

            if let Some(count) = properties.count {
                if count < 1000 {
                    return true;
                }
            }

            if let Some(until) = properties.until {
                if (until.naive_utc() - starts_at.datetime.naive_utc()).num_days() <= 36525 {
                    return true;
                }
            }

            false
        });

        let dt_of_last_occurrence = if is_bounded {
            // For bounded RRULEs calculate the date of the last occurrence
            // Still limiting the iterations - just in case
            rrule_set.into_iter().take(36525).last().ok_or_else(|| {
                DefaultApiError::BadRequest("recurrence_pattern does not yield any dates".into())
            })?
        } else {
            // For RRULEs for which calculating the last occurrence might take too
            // long, as they run forever or into the very far future, just take a
            // date 100 years from the start date (or if invalid fall back to the chrono MAX DATE)
            starts_at
                .datetime
                .with_year(ends_at_dt.year() + 100)
                .unwrap_or(chrono::MAX_DATETIME)
                .with_timezone(&ends_at.timezone.0)
        };

        Ok((
            Some(duration_secs as i32),
            dt_of_last_occurrence,
            ends_at.timezone,
        ))
    } else {
        Ok((None, ends_at.to_datetime_tz(), ends_at.timezone))
    }
}

/// Helper trait to to reduce boilerplate in the single route handlers
///
/// Bundles multiple resources into groups.
pub trait EventPoliciesBuilderExt {
    fn event_read_access(self, event_id: EventId) -> Self;
    fn event_write_access(self, event_id: EventId) -> Self;

    fn event_invite_invitee_access(self, event_id: EventId) -> Self;
}

impl<T> EventPoliciesBuilderExt for PoliciesBuilder<GrantingAccess<T>>
where
    T: IsSubject + Clone,
{
    /// GET access to the event and related endpoints.
    /// PUT and DELETE to the event_favorites endpoint.
    fn event_read_access(self, event_id: EventId) -> Self {
        self.add_resource(event_id.resource_id(), [AccessMethod::Get])
            .add_resource(
                event_id.resource_id().with_suffix("/instances"),
                [AccessMethod::Get],
            )
            .add_resource(
                event_id.resource_id().with_suffix("/instances/*"),
                [AccessMethod::Get],
            )
            .add_resource(
                event_id.resource_id().with_suffix("/invites"),
                [AccessMethod::Get],
            )
            .add_resource(
                format!("/users/me/event_favorites/{event_id}"),
                [AccessMethod::Put, AccessMethod::Delete],
            )
    }

    /// PATCH and DELETE to the event
    /// POST to reschedule and invites of the event
    /// PATCH to instances
    /// DELETE to invites
    fn event_write_access(self, event_id: EventId) -> Self {
        self.add_resource(
            event_id.resource_id(),
            [AccessMethod::Patch, AccessMethod::Delete],
        )
        .add_resource(
            event_id.resource_id().with_suffix("/reschedule"),
            [AccessMethod::Post],
        )
        .add_resource(
            event_id.resource_id().with_suffix("/instances/*"),
            [AccessMethod::Patch],
        )
        .add_resource(
            event_id.resource_id().with_suffix("/invites"),
            [AccessMethod::Post],
        )
        .add_resource(
            event_id.resource_id().with_suffix("/invites/*"),
            [AccessMethod::Delete],
        )
    }

    /// PATCH and DELETE to event invite
    fn event_invite_invitee_access(self, event_id: EventId) -> Self {
        self.add_resource(
            format!("/events/{event_id}/invite"),
            [AccessMethod::Patch, AccessMethod::Delete],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use db_storage::events::TimeZone;
    use db_storage::rooms::RoomId;
    use db_storage::users::UserId;
    use std::time::SystemTime;
    use test_util::assert_eq_json;
    use uuid::Uuid;

    #[test]
    fn event_resource_serialize() {
        let unix_epoch: DateTime<Utc> = SystemTime::UNIX_EPOCH.into();

        let user_profile = PublicUserProfile {
            id: UserId::from(Uuid::nil()),
            email: "test@example.org".into(),
            title: "".into(),
            firstname: "Test".into(),
            lastname: "Test".into(),
            display_name: "Tester".into(),
            avatar_url: "https://example.org/avatar".into(),
        };

        let event_resource = EventResource {
            id: EventId::from(Uuid::nil()),
            created_by: user_profile.clone(),
            created_at: unix_epoch,
            updated_by: user_profile.clone(),
            updated_at: unix_epoch,
            title: "Event title".into(),
            description: "Event description".into(),
            room: EventRoomInfo {
                id: RoomId::from(Uuid::nil()),
                password: None,
                sip_tel: None,
                sip_uri: None,
                sip_id: None,
                sip_password: None,
            },
            invitees_truncated: false,
            invitees: vec![EventInvitee {
                profile: user_profile,
                status: EventInviteStatus::Accepted,
            }],
            is_time_independent: false,
            is_all_day: Some(false),
            starts_at: Some(DateTimeTz {
                datetime: unix_epoch,
                timezone: TimeZone(Tz::Europe__Berlin),
            }),
            ends_at: Some(DateTimeTz {
                datetime: unix_epoch,
                timezone: TimeZone(Tz::Europe__Berlin),
            }),
            recurrence_pattern: vec![],
            type_: EventType::Single,
            status: EventStatus::Ok,
            invite_status: EventInviteStatus::Accepted,
            is_favorite: false,
        };

        assert_eq_json!(
            event_resource,
            {
                "id": "00000000-0000-0000-0000-000000000000",
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
                "title": "Event title",
                "description": "Event description",
                "room": {
                    "id": "00000000-0000-0000-0000-000000000000"
                },
                "invitees_truncated": false,
                "invitees": [
                    {
                        "profile": {
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
                "is_time_independent": false,
                "is_all_day": false,
                "starts_at": {
                    "datetime": "1970-01-01T00:00:00Z",
                    "timezone": "Europe/Berlin"
                },
                "ends_at": {
                    "datetime": "1970-01-01T00:00:00Z",
                    "timezone": "Europe/Berlin"
                },
                "type": "single",
                "status": "ok",
                "invite_status": "accepted",
                "is_favorite": false
            }
        );
    }

    #[test]
    fn event_resource_time_independent_serialize() {
        let unix_epoch: DateTime<Utc> = SystemTime::UNIX_EPOCH.into();

        let user_profile = PublicUserProfile {
            id: UserId::from(Uuid::nil()),
            email: "test@example.org".into(),
            title: "".into(),
            firstname: "Test".into(),
            lastname: "Test".into(),
            display_name: "Tester".into(),
            avatar_url: "https://example.org/avatar".into(),
        };

        let event_resource = EventResource {
            id: EventId::from(Uuid::nil()),
            created_by: user_profile.clone(),
            created_at: unix_epoch,
            updated_by: user_profile.clone(),
            updated_at: unix_epoch,
            title: "Event title".into(),
            description: "Event description".into(),
            room: EventRoomInfo {
                id: RoomId::from(Uuid::nil()),
                password: None,
                sip_tel: None,
                sip_uri: None,
                sip_id: None,
                sip_password: None,
            },
            invitees_truncated: false,
            invitees: vec![EventInvitee {
                profile: user_profile,
                status: EventInviteStatus::Accepted,
            }],
            is_time_independent: true,
            is_all_day: None,
            starts_at: None,
            ends_at: None,
            recurrence_pattern: vec![],
            type_: EventType::Single,
            status: EventStatus::Ok,
            invite_status: EventInviteStatus::Accepted,
            is_favorite: true,
        };

        assert_eq_json!(
            event_resource,
            {
                "id": "00000000-0000-0000-0000-000000000000",
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
                "title": "Event title",
                "description": "Event description",
                "room": {
                    "id": "00000000-0000-0000-0000-000000000000"
                },
                "invitees_truncated": false,
                "invitees": [
                    {
                        "profile": {
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
                "is_time_independent": true,
                "type": "single",
                "status": "ok",
                "invite_status": "accepted",
                "is_favorite": true
            }
        );
    }

    #[test]
    fn event_exception_serialize() {
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

        let instance = EventExceptionResource {
            id: EventAndInstanceId(event_id, instance_id),
            recurring_event_id: event_id,
            instance_id,
            created_by: user_profile.clone(),
            created_at: unix_epoch,
            updated_by: user_profile,
            updated_at: unix_epoch,
            title: Some("Instance title".into()),
            description: Some("Instance description".into()),
            is_all_day: Some(false),
            starts_at: Some(DateTimeTz {
                datetime: unix_epoch,
                timezone: TimeZone(Tz::Europe__Berlin),
            }),
            ends_at: Some(DateTimeTz {
                datetime: unix_epoch,
                timezone: TimeZone(Tz::Europe__Berlin),
            }),
            original_starts_at: DateTimeTz {
                datetime: unix_epoch,
                timezone: TimeZone(Tz::Europe__Berlin),
            },
            type_: EventType::Exception,
            status: EventStatus::Ok,
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
                "is_all_day": false,
                "starts_at": {
                    "datetime": "1970-01-01T00:00:00Z",
                    "timezone": "Europe/Berlin"
                },
                "ends_at": {
                    "datetime": "1970-01-01T00:00:00Z",
                    "timezone": "Europe/Berlin"
                },
                "original_starts_at": {
                    "datetime": "1970-01-01T00:00:00Z",
                    "timezone": "Europe/Berlin"
                },
                "type": "exception",
                "status": "ok",
            }
        );
    }
}
