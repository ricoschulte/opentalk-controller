// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use std::sync::Arc;

use super::cursor::Cursor;
use super::request::default_pagination_per_page;
use super::response::error::ValidationErrorEntry;
use super::response::{ApiError, NoContent, CODE_VALUE_REQUIRED};
use super::users::{email_to_libravatar_url, PublicUserProfile, UnregisteredUser};
use super::{ApiResponse, DefaultApiResult, PagePaginationQuery};
use crate::api::v1::response::CODE_IGNORED_VALUE;
use crate::api::v1::rooms::RoomsPoliciesBuilderExt;
use crate::api::v1::util::comma_separated;
use crate::api::v1::util::{deserialize_some, GetUserProfilesBatched};
use crate::services::{
    ExternalMailRecipient, MailRecipient, MailService, RegisteredMailRecipient,
    UnregisteredMailRecipient,
};
use crate::settings::SharedSettingsActix;
use actix_web::web::{Data, Json, Path, Query, ReqData};
use actix_web::{delete, get, patch, post, Either};
use chrono::{DateTime, Datelike, NaiveTime, TimeZone as _, Utc};
use chrono_tz::Tz;
use controller_shared::settings::Settings;
use database::{Db, DbConnection};
use db_storage::events::{
    email_invites::EventEmailInvite, Event, EventException, EventExceptionKind, EventId,
    EventInvite, EventInviteStatus, NewEvent, TimeZone, UpdateEvent,
};
use db_storage::invites::Invite;
use db_storage::rooms::{NewRoom, Room, RoomId, UpdateRoom};
use db_storage::sip_configs::{NewSipConfig, SipConfig};
use db_storage::tenants::Tenant;
use db_storage::users::User;
use keycloak_admin::KeycloakAdminClient;
use kustos::policies_builder::{GrantingAccess, PoliciesBuilder};
use kustos::prelude::{AccessMethod, IsSubject};
use kustos::{Authz, Resource, ResourceId};
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
        write!(formatter, "timestamp in '{UTC_DT_FORMAT}' format")
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    pub fn ends_at_of(event: &Event) -> Option<Self> {
        event.ends_at_of_first_occurrence().map(|(dt, tz)| Self {
            datetime: dt,
            timezone: tz,
        })
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

    /// Flag indicating whether the event is ad-hoc created.
    pub is_adhoc: bool,

    /// Type of event
    ///
    /// Time independent events or events without recurrence are `single` while recurring events are `recurring`
    #[serde(rename = "type")]
    pub type_: EventType,

    /// The invite status of the current user for this event
    pub invite_status: EventInviteStatus,

    /// Is this event in the current user's favorite list?
    pub is_favorite: bool,

    /// Can the current user edit this resource
    pub can_edit: bool,
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

    /// Can the current user edit this resource
    pub can_edit: bool,
}

impl EventExceptionResource {
    pub fn from_db(
        exception: EventException,
        created_by: PublicUserProfile,
        can_edit: bool,
    ) -> Self {
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
            can_edit,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum EventInviteeProfile {
    Registered(PublicUserProfile),
    Unregistered(UnregisteredUser),
    Email(EmailOnlyUser),
}

#[derive(Debug, Clone, Serialize)]
pub struct EmailOnlyUser {
    pub email: String,
    pub avatar_url: String,
}

/// Invitee to an event
///
///  Contains user profile and invitee status
#[derive(Debug, Clone, Serialize)]
pub struct EventInvitee {
    pub profile: EventInviteeProfile,
    pub status: EventInviteStatus,
}

impl EventInvitee {
    fn from_invite_with_user(invite: EventInvite, user: User, settings: &Settings) -> EventInvitee {
        EventInvitee {
            profile: EventInviteeProfile::Registered(PublicUserProfile::from_db(settings, user)),
            status: invite.status,
        }
    }

    fn from_email_invite(invite: EventEmailInvite, settings: &Settings) -> EventInvitee {
        let avatar_url = email_to_libravatar_url(&settings.avatar.libravatar_url, &invite.email);
        EventInvitee {
            profile: EventInviteeProfile::Email(EmailOnlyUser {
                email: invite.email,
                avatar_url,
            }),
            status: EventInviteStatus::Pending,
        }
    }
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

    /// Flag to check if the room has a waiting room enabled
    pub waiting_room: bool,

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
            password: room.password,
            waiting_room: room.waiting_room,
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

    /// Should the created event have a waiting room?
    #[serde(default)]
    pub waiting_room: bool,

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

    /// Is this an ad-hoc chatroom?
    #[serde(default)]
    pub is_adhoc: bool,
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

    new_event.validate()?;

    let event_resource = crate::block(move || {
        let mut conn = db.get_conn()?;

        // simplify logic by splitting the event creation
        // into two paths: time independent and time dependent
        match new_event {
            PostEventsBody {
                title,
                description,
                password,
                waiting_room,
                is_time_independent: true,
                is_all_day: None,
                starts_at: None,
                ends_at: None,
                recurrence_pattern,
                is_adhoc,
            } if recurrence_pattern.is_empty() => {
                create_time_independent_event(
                    &settings,
                    &mut conn,
                    current_user,
                    title,
                    description,
                    password,
                    waiting_room,
                    is_adhoc
                )
            }
            PostEventsBody {
                title,
                description,
                password,
                waiting_room,
                is_time_independent: false,
                is_all_day: Some(is_all_day),
                starts_at: Some(starts_at),
                ends_at: Some(ends_at),
                recurrence_pattern,
                is_adhoc,
            } => {
                create_time_dependent_event(
                    &settings,
                    &mut conn,
                    current_user,
                    title,
                    description,
                    password,
                    waiting_room,
                    is_all_day,
                    starts_at,
                    ends_at,
                    recurrence_pattern,
                    is_adhoc,
                )
            }
            new_event => {
                let msg = if new_event.is_time_independent {
                    "time independent events must not have is_all_day, starts_at, ends_at or recurrence_pattern set"
                } else {
                    "time dependent events must have title, description, is_all_day, starts_at and ends_at set"
                };

                Err(ApiError::bad_request().with_message(msg))
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

    authz.add_policies(policies).await?;

    Ok(ApiResponse::new(event_resource))
}

/// Part of `POST /events` endpoint
#[allow(clippy::too_many_arguments)]
fn create_time_independent_event(
    settings: &Settings,
    conn: &mut DbConnection,
    current_user: User,
    title: String,
    description: String,
    password: Option<String>,
    waiting_room: bool,
    is_adhoc: bool,
) -> Result<EventResource, ApiError> {
    let room = NewRoom {
        created_by: current_user.id,
        password,
        waiting_room,
        tenant_id: current_user.tenant_id,
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
        is_adhoc,
        tenant_id: current_user.tenant_id,
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
        invite_status: EventInviteStatus::Accepted,
        is_favorite: false,
        can_edit: true, // just created by the current user
        is_adhoc,
    })
}

/// Part of `POST /events` endpoint
#[allow(clippy::too_many_arguments)]
fn create_time_dependent_event(
    settings: &Settings,
    conn: &mut DbConnection,
    current_user: User,
    title: String,
    description: String,
    password: Option<String>,
    waiting_room: bool,
    is_all_day: bool,
    starts_at: DateTimeTz,
    ends_at: DateTimeTz,
    recurrence_pattern: Vec<String>,
    is_adhoc: bool,
) -> Result<EventResource, ApiError> {
    let recurrence_pattern = recurrence_array_to_string(recurrence_pattern);

    let (duration_secs, ends_at_dt, ends_at_tz) =
        parse_event_dt_params(is_all_day, starts_at, ends_at, &recurrence_pattern)?;

    let room = NewRoom {
        created_by: current_user.id,
        password,
        waiting_room,
        tenant_id: current_user.tenant_id,
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
        is_adhoc,
        tenant_id: current_user.tenant_id,
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
        is_time_independent: event.is_time_independent,
        is_all_day: event.is_all_day,
        starts_at: Some(starts_at),
        ends_at: Some(ends_at),
        recurrence_pattern: recurrence_string_to_array(event.recurrence_pattern),
        type_: if event.is_recurring.unwrap_or_default() {
            EventType::Recurring
        } else {
            EventType::Single
        },
        invite_status: EventInviteStatus::Accepted,
        is_favorite: false,
        can_edit: true, // just created by the current user
        is_adhoc,
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

    /// Filter the events by invite status
    #[serde(default)]
    #[serde(deserialize_with = "comma_separated")]
    invite_status: Vec<EventInviteStatus>,

    /// How many events to return per page
    per_page: Option<i64>,

    /// Cursor token to get the next page of events
    ///
    /// Returned by the endpoint if the maximum number of events per page has been hit
    after: Option<Cursor<GetEventsCursorData>>,

    /// Only get events that are either marked as adhoc or non-adhoc
    ///
    /// If present, all adhoc events will be returned when `true`, all non-adhoc
    /// events will be returned when `false`. If not present, all events will
    /// be returned regardless of their `adhoc` flag value.
    adhoc: Option<bool>,

    /// Only get events that are either time-independent or time-dependent
    ///
    /// If present, all time-independent events will be returned when `true`,
    /// all time-dependent events will be returned when `false`. If absent,
    /// all events will be returned regardless of their time dependency.
    time_independent: Option<bool>,
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

struct GetPaginatedEventsData {
    event_resources: Vec<EventOrException>,
    before: Option<String>,
    after: Option<String>,
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
    kc_admin_client: Data<KeycloakAdminClient>,
    current_tenant: ReqData<Tenant>,
    current_user: ReqData<User>,
    query: Query<GetEventsQuery>,
) -> DefaultApiResult<Vec<EventOrException>> {
    let settings = settings.load_full();
    let current_user = current_user.into_inner();
    let query = query.into_inner();

    let kc_admin_client_ref = &kc_admin_client;

    let events_data = crate::block(move || -> Result<GetPaginatedEventsData, ApiError> {
        let per_page = query
            .per_page
            .unwrap_or_else(default_pagination_per_page)
            .clamp(1, 100);

        let mut users = GetUserProfilesBatched::new();

        let get_events_cursor = query
            .after
            .map(|cursor| db_storage::events::GetEventsCursor {
                from_id: cursor.event_id,
                from_created_at: cursor.event_created_at,
                from_starts_at: cursor.event_starts_at,
            });

        let mut conn = db.get_conn()?;

        let events = Event::get_all_for_user_paginated(
            &mut conn,
            &current_user,
            query.favorites,
            query.invite_status,
            query.time_min,
            query.time_max,
            query.adhoc,
            query.time_independent,
            get_events_cursor,
            per_page,
        )?;

        for (event, _, _, _, exceptions, _) in &events {
            users.add(event);
            users.add(exceptions);
        }

        let users = users.fetch(&settings, &mut conn)?;

        let event_refs: Vec<&Event> = events.iter().map(|(event, ..)| event).collect();

        // Build list of event invites with user, grouped by events
        let invites_with_users_grouped_by_event = if query.invitees_max == 0 {
            // Do not query event invites if invitees_max is zero, instead create dummy value
            (0..events.len()).map(|_| Vec::new()).collect()
        } else {
            EventInvite::get_for_events(&mut conn, &event_refs)?
        };

        // Build list of additional email event invites, grouped by events
        let email_invites_grouped_by_event = if query.invitees_max == 0 {
            // Do not query email event invites if invitees_max is zero, instead create dummy value
            (0..events.len()).map(|_| Vec::new()).collect()
        } else {
            EventEmailInvite::get_for_events(&mut conn, &event_refs)?
        };

        type InvitesByEvent = Vec<(Vec<(EventInvite, User)>, Vec<EventEmailInvite>)>;
        let invites_grouped_by_event: InvitesByEvent = invites_with_users_grouped_by_event
            .into_iter()
            .zip(email_invites_grouped_by_event)
            .collect();

        let mut event_resources = vec![];

        let mut ret_cursor_data = None;

        for (
            (event, invite, room, sip_config, exceptions, is_favorite),
            (mut invites_with_user, mut email_invites),
        ) in events.into_iter().zip(invites_grouped_by_event)
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

            let invitees_truncated = query.invitees_max == 0
                || (invites_with_user.len() + email_invites.len()) > query.invitees_max as usize;

            invites_with_user.truncate(query.invitees_max as usize);
            let email_invitees_max = query.invitees_max - invites_with_user.len().max(0) as u32;
            email_invites.truncate(email_invitees_max as usize);

            let registered_invitees_iter = invites_with_user
                .into_iter()
                .map(|(invite, user)| EventInvitee::from_invite_with_user(invite, user, &settings));

            let unregistered_invitees_iter = email_invites
                .into_iter()
                .map(|invite| EventInvitee::from_email_invite(invite, &settings));

            let invitees = registered_invitees_iter
                .chain(unregistered_invitees_iter)
                .collect();

            let starts_at = DateTimeTz::starts_at_of(&event);
            let ends_at = DateTimeTz::ends_at_of(&event);

            let can_edit = can_edit(&event, &current_user);

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
                invite_status,
                is_favorite,
                can_edit,
                is_adhoc: event.is_adhoc,
            }));

            for exception in exceptions {
                let created_by = users.get(exception.created_by);

                event_resources.push(EventOrException::Exception(
                    EventExceptionResource::from_db(exception, created_by, can_edit),
                ));
            }
        }

        Ok(GetPaginatedEventsData {
            event_resources,
            before: None,
            after: ret_cursor_data.map(|c| Cursor(c).to_base64()),
        })
    })
    .await??;

    let resource_mapping_futures = events_data
        .event_resources
        .into_iter()
        .map(|resource| async {
            match resource {
                EventOrException::Event(inner) => EventOrException::Event(EventResource {
                    invitees: enrich_invitees_from_keycloak(
                        kc_admin_client_ref,
                        &current_tenant,
                        inner.invitees,
                    )
                    .await,
                    ..inner
                }),
                EventOrException::Exception(inner) => EventOrException::Exception(inner),
            }
        });
    let event_resources = futures::future::join_all(resource_mapping_futures).await;

    Ok(ApiResponse::new(event_resources)
        .with_cursor_pagination(events_data.before, events_data.after))
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
    kc_admin_client: Data<KeycloakAdminClient>,
    current_tenant: ReqData<Tenant>,
    current_user: ReqData<User>,
    event_id: Path<EventId>,
    query: Query<GetEventQuery>,
) -> DefaultApiResult<EventResource> {
    let settings = settings.load_full();
    let event_id = event_id.into_inner();
    let query = query.into_inner();

    let event_resource = crate::block(move || -> Result<EventResource, ApiError> {
        let mut conn = db.get_conn()?;

        let (event, invite, room, sip_config, is_favorite) =
            Event::get_with_invite_and_room(&mut conn, current_user.id, event_id)?;
        let (invitees, invitees_truncated) =
            get_invitees_for_event(&settings, &mut conn, event_id, query.invitees_max)?;

        let users = GetUserProfilesBatched::new()
            .add(&event)
            .fetch(&settings, &mut conn)?;

        let starts_at = DateTimeTz::starts_at_of(&event);
        let ends_at = DateTimeTz::ends_at_of(&event);

        let can_edit = can_edit(&event, &current_user);

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
            invite_status: invite
                .map(|inv| inv.status)
                .unwrap_or(EventInviteStatus::Accepted),
            is_favorite,
            can_edit,
            is_adhoc: event.is_adhoc,
        };

        Ok(event_resource)
    })
    .await??;

    let event_resource = EventResource {
        invitees: enrich_invitees_from_keycloak(
            &kc_admin_client,
            &current_tenant,
            event_resource.invitees,
        )
        .await,
        ..event_resource
    };

    Ok(ApiResponse::new(event_resource))
}

/// Path query parameters for the `PATCH /events/{event_id}` endpoint
#[derive(Debug, Deserialize)]
pub struct PatchEventQuery {
    /// Maximum number of invitees to include inside the event
    #[serde(default)]
    invitees_max: i64,

    /// Flag to disable email notification
    #[serde(default)]
    suppress_email_notification: bool,
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

    /// Patch the password of the event's room
    #[validate(length(min = 1, max = 255))]
    #[serde(default, deserialize_with = "deserialize_some")]
    password: Option<Option<String>>,

    /// Patch the presence of a waiting room
    waiting_room: Option<bool>,

    /// Patch the adhoc flag.
    is_adhoc: Option<bool>,

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

impl PatchEventBody {
    fn is_empty(&self) -> bool {
        let PatchEventBody {
            title,
            description,
            password,
            waiting_room,
            is_adhoc,
            is_time_independent,
            is_all_day,
            starts_at,
            ends_at,
            recurrence_pattern,
        } = self;

        title.is_none()
            && description.is_none()
            && password.is_none()
            && waiting_room.is_none()
            && is_adhoc.is_none()
            && is_time_independent.is_none()
            && is_all_day.is_none()
            && starts_at.is_none()
            && ends_at.is_none()
            && recurrence_pattern.is_empty()
    }

    // special case to only patch the events room
    fn only_modifies_room(&self) -> bool {
        let PatchEventBody {
            title,
            description,
            password,
            waiting_room,
            is_time_independent,
            is_all_day,
            starts_at,
            ends_at,
            recurrence_pattern,
            is_adhoc,
        } = self;

        title.is_none()
            && description.is_none()
            && is_time_independent.is_none()
            && is_all_day.is_none()
            && starts_at.is_none()
            && ends_at.is_none()
            && recurrence_pattern.is_empty()
            && is_adhoc.is_none()
            && (password.is_some() || waiting_room.is_some())
    }
}

struct UpdateNotificationValues {
    pub tenant: Tenant,
    pub created_by: User,
    pub event: Event,
    pub room: Room,
    pub sip_config: Option<SipConfig>,
    pub invited_users: Vec<MailRecipient>,
    pub invite_for_room: Invite,
}

/// API Endpoint `PATCH /events/{event_id}`
///
/// See documentation of [`PatchEventBody`] for more infos
///
/// Patches which modify the event in a way that would invalidate existing
/// exceptions (e.g. by changing the recurrence rule or time dependence)
/// will have all exceptions deleted
#[allow(clippy::too_many_arguments)]
#[patch("/events/{event_id}")]
pub async fn patch_event(
    settings: SharedSettingsActix,
    db: Data<Db>,
    kc_admin_client: Data<KeycloakAdminClient>,
    current_tenant: ReqData<Tenant>,
    current_user: ReqData<User>,
    event_id: Path<EventId>,
    query: Query<PatchEventQuery>,
    patch: Json<PatchEventBody>,
    mail_service: Data<MailService>,
) -> Result<Either<ApiResponse<EventResource>, NoContent>, ApiError> {
    let patch = patch.into_inner();

    if patch.is_empty() {
        return Ok(Either::Right(NoContent));
    }

    patch.validate()?;

    let settings = settings.load_full();
    let current_tenant = current_tenant.into_inner();
    let current_user = current_user.into_inner();
    let event_id = event_id.into_inner();
    let query = query.into_inner();
    let mail_service = mail_service.into_inner();

    let send_email_notification = !query.suppress_email_notification;

    let (event_resource, notification_values) = crate::block({
        let current_tenant = current_tenant.clone();

        move || -> Result<(EventResource, UpdateNotificationValues), ApiError> {
            let mut conn = db.get_conn()?;

            let (event, invite, room, sip_config, is_favorite) =
                Event::get_with_invite_and_room(&mut conn, current_user.id, event_id)?;

            let room = if patch.password.is_some() || patch.waiting_room.is_some() {
                // Update the event's room if at least one of the fields is set
                UpdateRoom {
                    password: patch.password.clone(),
                    waiting_room: patch.waiting_room,
                }
                .apply(&mut conn, event.room)?
            } else {
                room
            };

            let created_by = if event.created_by == current_user.id {
                current_user.clone()
            } else {
                User::get(&mut conn, event.created_by)?
            };

            // Special case: if the patch only modifies the password do not update the event
            let event = if patch.only_modifies_room() {
                event
            } else {
                let update_event = match (event.is_time_independent, patch.is_time_independent) {
                    (true, Some(false)) => {
                        // The patch changes the event from an time-independent event
                        // to a time dependent event
                        patch_event_change_to_time_dependent(&current_user, patch)?
                    }
                    (true, _) | (false, Some(true)) => {
                        // The patch will modify an time-independent event or
                        // change an event to a time-independent event
                        patch_time_independent_event(&mut conn, &current_user, &event, patch)?
                    }
                    _ => {
                        // The patch modifies an time dependent event
                        patch_time_dependent_event(&mut conn, &current_user, &event, patch)?
                    }
                };

                update_event.apply(&mut conn, event_id)?
            };

            let invited_users = get_invited_mail_recipients_for_event(&mut conn, event_id)?;
            let invite_for_room = Invite::get_first_for_room(&mut conn, room.id, current_user.id)?;
            let notification_values = UpdateNotificationValues {
                tenant: current_tenant,
                created_by: created_by.clone(),
                event: event.clone(),
                room: room.clone(),
                sip_config: sip_config.clone(),
                invited_users,
                invite_for_room,
            };

            let (invitees, invitees_truncated) =
                get_invitees_for_event(&settings, &mut conn, event_id, query.invitees_max)?;

            let starts_at = DateTimeTz::starts_at_of(&event);
            let ends_at = DateTimeTz::ends_at_of(&event);

            let can_edit = can_edit(&event, &current_user);

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
                invite_status: invite
                    .map(|inv| inv.status)
                    .unwrap_or(EventInviteStatus::Accepted),
                is_favorite,
                can_edit,
                is_adhoc: event.is_adhoc,
            };

            Ok((event_resource, notification_values))
        }
    })
    .await??;

    if send_email_notification {
        notify_invitees_about_update(notification_values, mail_service, &kc_admin_client).await;
    }

    let event_resource = EventResource {
        invitees: enrich_invitees_from_keycloak(
            &kc_admin_client,
            &current_tenant,
            event_resource.invitees,
        )
        .await,
        ..event_resource
    };

    Ok(Either::Left(ApiResponse::new(event_resource)))
}

/// Part of `PATCH /events/{event_id}` (see [`patch_event`])
///
/// Notify invited users about the event update
async fn notify_invitees_about_update(
    notification_values: UpdateNotificationValues,
    mail_service: Arc<MailService>,
    kc_admin_client: &Data<KeycloakAdminClient>,
) {
    for invited_user in notification_values.invited_users {
        let invited_user =
            enrich_from_keycloak(invited_user, &notification_values.tenant, kc_admin_client).await;

        if let Err(e) = mail_service
            .send_event_update(
                notification_values.created_by.clone(),
                notification_values.event.clone(),
                notification_values.room.clone(),
                notification_values.sip_config.clone(),
                invited_user,
                notification_values.invite_for_room.id.to_string(),
            )
            .await
        {
            log::error!("Failed to send event update with MailService, {}", e);
        }
    }
}

/// Part of `PATCH /events/{event_id}` (see [`patch_event`])
///
/// Patch event which is time independent into a time dependent event
fn patch_event_change_to_time_dependent(
    current_user: &User,
    patch: PatchEventBody,
) -> Result<UpdateEvent, ApiError> {
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
            is_adhoc: patch.is_adhoc,
        })
    } else {
        const MSG: Option<&str> = Some("Must be provided when changing to time dependent events");

        let mut entries = vec![];

        if patch.is_all_day.is_some() {
            entries.push(ValidationErrorEntry::new(
                "is_all_day",
                CODE_VALUE_REQUIRED,
                MSG,
            ))
        }

        if patch.starts_at.is_some() {
            entries.push(ValidationErrorEntry::new(
                "starts_at",
                CODE_VALUE_REQUIRED,
                MSG,
            ))
        }

        if patch.ends_at.is_some() {
            entries.push(ValidationErrorEntry::new(
                "ends_at",
                CODE_VALUE_REQUIRED,
                MSG,
            ))
        }

        Err(ApiError::unprocessable_entities(entries))
    }
}

/// Part of `PATCH /events/{event_id}` (see [`patch_event`])
///
/// Patch event which is time dependent into a time independent event
fn patch_time_independent_event(
    conn: &mut DbConnection,
    current_user: &User,
    event: &Event,
    patch: PatchEventBody,
) -> Result<UpdateEvent, ApiError> {
    if patch.is_all_day.is_some() || patch.starts_at.is_some() || patch.ends_at.is_some() {
        const MSG: Option<&str> = Some("Value would be ignored in this request");

        let mut entries = vec![];

        if patch.is_all_day.is_some() {
            entries.push(ValidationErrorEntry::new(
                "is_all_day",
                CODE_IGNORED_VALUE,
                MSG,
            ))
        }

        if patch.starts_at.is_some() {
            entries.push(ValidationErrorEntry::new(
                "starts_at",
                CODE_IGNORED_VALUE,
                MSG,
            ))
        }

        if patch.ends_at.is_some() {
            entries.push(ValidationErrorEntry::new(
                "ends_at",
                CODE_IGNORED_VALUE,
                MSG,
            ))
        }

        return Err(ApiError::unprocessable_entities(entries));
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
        is_adhoc: patch.is_adhoc,
    })
}

/// Part of `PATCH /events/{event_id}` (see [`patch_event`])
///
/// Patch fields on an time dependent event (without changing the time dependence field)
fn patch_time_dependent_event(
    conn: &mut DbConnection,
    current_user: &User,
    event: &Event,
    patch: PatchEventBody,
) -> Result<UpdateEvent, ApiError> {
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
        is_adhoc: patch.is_adhoc,
        recurrence_pattern: Some(recurrence_pattern),
    })
}

struct CancellationNotificationValues {
    pub tenant: Tenant,
    pub created_by: User,
    pub event: Event,
    pub room: Room,
    pub sip_config: Option<SipConfig>,
    pub invited_users: Vec<MailRecipient>,
}

/// Query parameters for the `DELETE /events/{event_id}` endpoint
#[derive(Debug, Deserialize)]
pub struct DeleteEventQuery {
    /// Flag to disable email notification
    #[serde(default)]
    suppress_email_notification: bool,
}

/// API Endpoint `POST /events/{event_id}`
#[delete("/events/{event_id}")]
#[allow(clippy::too_many_arguments)]
pub async fn delete_event(
    db: Data<Db>,
    kc_admin_client: Data<KeycloakAdminClient>,
    current_tenant: ReqData<Tenant>,
    current_user: ReqData<User>,
    authz: Data<Authz>,
    query: Query<DeleteEventQuery>,
    event_id: Path<EventId>,
    mail_service: Data<MailService>,
) -> Result<NoContent, ApiError> {
    let current_user = current_user.into_inner();
    let event_id = event_id.into_inner();
    let mail_service = mail_service.into_inner();

    let send_email_notification = !query.suppress_email_notification;

    let notification_values = crate::block(
        move || -> Result<CancellationNotificationValues, ApiError> {
            let mut conn = db.get_conn()?;

            // TODO(w.rabl) Further DB access optimization (replacing call to get_with_invite_and_room)?
            let (event, _invite, room, sip_config, _is_favorite) =
                Event::get_with_invite_and_room(&mut conn, current_user.id, event_id)?;

            let created_by = if event.created_by == current_user.id {
                current_user
            } else {
                User::get(&mut conn, event.created_by)?
            };

            let invited_users = get_invited_mail_recipients_for_event(&mut conn, event_id)?;

            Event::delete_by_id(&mut conn, event_id)?;

            let notification_values = CancellationNotificationValues {
                tenant: current_tenant.into_inner(),
                created_by,
                event,
                room,
                sip_config,
                invited_users,
            };
            Ok(notification_values)
        },
    )
    .await??;

    if send_email_notification {
        notify_invitees_about_delete(notification_values, mail_service, &kc_admin_client).await;
    }

    let resources = associated_resource_ids(event_id);

    authz.remove_explicit_resources(resources).await?;

    Ok(NoContent)
}

/// Part of `DELETE /events/{event_id}` (see [`patch_event`])
///
/// Notify invited users about the event update
async fn notify_invitees_about_delete(
    notification_values: CancellationNotificationValues,
    mail_service: Arc<MailService>,
    kc_admin_client: &Data<KeycloakAdminClient>,
) {
    for invited_user in notification_values.invited_users {
        let invited_user =
            enrich_from_keycloak(invited_user, &notification_values.tenant, kc_admin_client).await;

        if let Err(e) = mail_service
            .send_event_cancellation(
                notification_values.created_by.clone(),
                notification_values.event.clone(),
                notification_values.room.clone(),
                notification_values.sip_config.clone(),
                invited_user,
            )
            .await
        {
            log::error!("Failed to send event cancellation with MailService, {}", e);
        }
    }
}

pub(crate) fn associated_resource_ids(event_id: EventId) -> impl IntoIterator<Item = ResourceId> {
    [
        ResourceId::from(format!("/events/{event_id}")),
        ResourceId::from(format!("/events/{event_id}/instances")),
        ResourceId::from(format!("/events/{event_id}/instances/*")),
        ResourceId::from(format!("/events/{event_id}/invites")),
        ResourceId::from(format!("/events/{event_id}/invites/*")),
        ResourceId::from(format!("/events/{event_id}/invite")),
        ResourceId::from(format!("/events/{event_id}/reschedule")),
        ResourceId::from(format!("/users/me/event_favorites/{event_id}")),
    ]
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
    conn: &mut DbConnection,
    event_id: EventId,
    invitees_max: i64,
) -> database::Result<(Vec<EventInvitee>, bool)> {
    if invitees_max > 0 {
        // Get regular invitees up to the maximum invitee count specified.

        let (invites_with_user, total_invites_count) =
            EventInvite::get_for_event_paginated(conn, event_id, invitees_max, 1)?;

        let mut invitees: Vec<EventInvitee> = invites_with_user
            .into_iter()
            .map(|(invite, user)| EventInvitee::from_invite_with_user(invite, user, settings))
            .collect();

        let loaded_invites_count = invitees.len() as i64;
        let mut invitees_truncated = total_invites_count > loaded_invites_count;

        // Now add email invitees until the maximum total invitee count specified is reached.

        let invitees_max = invitees_max - loaded_invites_count;
        if invitees_max > 0 {
            let (email_invites, total_email_invites_count) =
                EventEmailInvite::get_for_event_paginated(conn, event_id, invitees_max, 1)?;

            let email_invitees: Vec<EventInvitee> = email_invites
                .into_iter()
                .map(|invite| EventInvitee::from_email_invite(invite, settings))
                .collect();

            let loaded_email_invites_count = email_invitees.len() as i64;
            invitees_truncated =
                invitees_truncated || (total_email_invites_count > loaded_email_invites_count);

            invitees.extend(email_invitees);
        }

        Ok((invitees, invitees_truncated))
    } else {
        Ok((vec![], true))
    }
}

fn get_invited_mail_recipients_for_event(
    conn: &mut DbConnection,
    event_id: EventId,
) -> database::Result<Vec<MailRecipient>> {
    // TODO(w.rabl) Further DB access optimization (replacing call to get_for_event_paginated)?
    let (invites_with_user, _) = EventInvite::get_for_event_paginated(conn, event_id, i64::MAX, 1)?;
    let user_invitees = invites_with_user.into_iter().map(|(_, user)| {
        MailRecipient::Registered(RegisteredMailRecipient {
            email: user.email,
            title: user.title,
            first_name: user.firstname,
            last_name: user.lastname,
            language: user.language,
        })
    });

    let (email_invites, _) =
        EventEmailInvite::get_for_event_paginated(conn, event_id, i64::MAX, 1)?;
    let email_invitees = email_invites.into_iter().map(|invitee| {
        MailRecipient::External(ExternalMailRecipient {
            email: invitee.email,
        })
    });

    let invitees = user_invitees.chain(email_invitees).collect();

    Ok(invitees)
}

async fn enrich_invitees_from_keycloak(
    kc_admin_client: &Data<KeycloakAdminClient>,
    current_tenant: &Tenant,
    invitees: Vec<EventInvitee>,
) -> Vec<EventInvitee> {
    let invitee_mapping_futures = invitees.into_iter().map(|invitee| async move {
        if let EventInviteeProfile::Email(profile_details) = invitee.profile {
            let user_for_email = kc_admin_client
                .get_user_for_email(
                    current_tenant.oidc_tenant_id.inner(),
                    profile_details.email.as_ref(),
                )
                .await
                .unwrap_or_default();

            if let Some(user) = user_for_email {
                let profile_details = UnregisteredUser {
                    email: profile_details.email,
                    firstname: user.first_name,
                    lastname: user.last_name,
                    avatar_url: profile_details.avatar_url,
                };
                EventInvitee {
                    profile: EventInviteeProfile::Unregistered(profile_details),
                    ..invitee
                }
            } else {
                EventInvitee {
                    profile: EventInviteeProfile::Email(profile_details),
                    ..invitee
                }
            }
        } else {
            invitee
        }
    });
    futures::future::join_all(invitee_mapping_futures).await
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
) -> Result<(), ApiError> {
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
) -> Result<(Option<i32>, DateTime<Tz>, TimeZone), ApiError> {
    const CODE_INVALID_EVENT: &str = "invalid_event";

    let starts_at_dt = starts_at.to_datetime_tz();
    let ends_at_dt = ends_at.to_datetime_tz();

    let duration_secs = (ends_at_dt - starts_at_dt).num_seconds();

    if duration_secs < 0 {
        return Err(ApiError::unprocessable_entity()
            .with_code(CODE_INVALID_EVENT)
            .with_message("ends_at must not be before starts_at"));
    }

    if is_all_day {
        let zero = NaiveTime::from_hms_opt(0, 0, 0).unwrap();

        if starts_at.datetime.time() != zero || ends_at.datetime.time() != zero {
            return Err(ApiError::unprocessable_entity()
                .with_code(CODE_INVALID_EVENT)
                .with_message(
                    "is_all_day requires starts_at/ends_at to be set at the start of the day",
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
                return Err(ApiError::unprocessable_entity()
                    .with_code(CODE_INVALID_EVENT)
                    .with_message("Invalid recurrence pattern"));
            }
        };

        if rrule_set
            .rrule
            .iter()
            .any(|rrule| rrule.get_properties().freq > Frequency::Daily)
        {
            return Err(ApiError::unprocessable_entity()
                .with_code(CODE_INVALID_EVENT)
                .with_message("Frequencies below 'DAILY' are not supported"));
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
                ApiError::unprocessable_entity()
                    .with_code(CODE_INVALID_EVENT)
                    .with_message("recurrence_pattern does not yield any dates")
            })?
        } else {
            // For RRULEs for which calculating the last occurrence might take too
            // long, as they run forever or into the very far future, just take a
            // date 100 years from the start date (or if invalid fall back to the chrono MAX DATE)
            starts_at
                .datetime
                .with_year(ends_at_dt.year() + 100)
                .unwrap_or(DateTime::<Utc>::MAX_UTC)
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

/// calculate if `user` can edit `event`
fn can_edit(event: &Event, user: &User) -> bool {
    // Its sufficient to check if the user created the event as here isn't currently a system which allows users to
    // grant write access to event
    event.created_by == user.id
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

async fn enrich_from_keycloak(
    recipient: MailRecipient,
    current_tenant: &Tenant,
    kc_admin_client: &Data<KeycloakAdminClient>,
) -> MailRecipient {
    if let MailRecipient::External(recipient) = recipient {
        let keycloak_user = kc_admin_client
            .get_user_for_email(
                current_tenant.oidc_tenant_id.inner(),
                recipient.email.as_ref(),
            )
            .await
            .unwrap_or_default();

        if let Some(keycloak_user) = keycloak_user {
            MailRecipient::Unregistered(UnregisteredMailRecipient {
                email: recipient.email,
                first_name: keycloak_user.first_name,
                last_name: keycloak_user.last_name,
            })
        } else {
            MailRecipient::External(recipient)
        }
    } else {
        recipient
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
            invite_status: EventInviteStatus::Accepted,
            is_favorite: false,
            can_edit: true,
            is_adhoc: false,
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
                "invite_status": "accepted",
                "is_favorite": false,
                "can_edit": true,
                "is_adhoc": false,
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
            is_time_independent: true,
            is_all_day: None,
            starts_at: None,
            ends_at: None,
            recurrence_pattern: vec![],
            type_: EventType::Single,
            invite_status: EventInviteStatus::Accepted,
            is_favorite: true,
            can_edit: false,
            is_adhoc: false,
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
                "is_time_independent": true,
                "type": "single",
                "invite_status": "accepted",
                "is_favorite": true,
                "can_edit": false,
                "is_adhoc": false,
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
                "can_edit": false,
            }
        );
    }
}
