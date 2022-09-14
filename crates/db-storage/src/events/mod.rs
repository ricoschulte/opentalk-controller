use crate::rooms::{Room, RoomId};
use crate::schema::{
    event_exceptions, event_favorites, event_invites, events, rooms, sip_configs, users,
};
use crate::sip_configs::SipConfig;
use crate::users::{User, UserId};
use crate::utils::HasUsers;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use database::{DatabaseError, DbConnection, Paginate, Result};
use diesel::associations::BelongsTo;
use diesel::backend::Backend;
use diesel::expression::AsExpression;
use diesel::pg::Pg;
use diesel::prelude::*;
use diesel::serialize::{self, Output};
use diesel::sql_types::{Nullable, Timestamptz, Uuid};
use diesel::types::{FromSql, IsNull, Record, ToSql};
use diesel::{
    deserialize, BoolExpressionMethods, ExpressionMethods, JoinOnDsl, NullableExpressionMethods,
    OptionalExtension, PgSortExpressionMethods, QueryDsl, Queryable, RunQueryDsl,
};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::str::{from_utf8, FromStr};

diesel_newtype! {
    #[derive(Copy)] EventId(uuid::Uuid) => diesel::sql_types::Uuid, "diesel::sql_types::Uuid", "/events/",
    #[derive(Copy)] EventSerialId(i64) => diesel::sql_types::BigInt, "diesel::sql_types::BigInt",

    #[derive(Copy)] EventExceptionId(uuid::Uuid) => diesel::sql_types::Uuid, "diesel::sql_types::Uuid",

    #[derive(Copy)] EventInviteId(uuid::Uuid) => diesel::sql_types::Uuid, "diesel::sql_types::Uuid"
}

pub mod email_invites;

#[derive(Debug, Copy, Clone, PartialEq, FromSqlRow, AsExpression, Serialize, Deserialize)]
#[sql_type = "diesel::sql_types::Text"]
pub struct TimeZone(pub chrono_tz::Tz);

impl ToSql<diesel::sql_types::Text, Pg> for TimeZone {
    fn to_sql<W: Write>(&self, out: &mut Output<W, Pg>) -> serialize::Result {
        write!(out, "{}", self.0)?;
        Ok(IsNull::No)
    }
}

impl FromSql<diesel::sql_types::Text, Pg> for TimeZone {
    fn from_sql(bytes: Option<&<Pg as Backend>::RawValue>) -> deserialize::Result<Self> {
        let bytes = bytes.ok_or("tried to deserialize Tz from None")?;
        let s = from_utf8(bytes)?;
        let tz = Tz::from_str(s)?;

        Ok(Self(tz))
    }
}

#[derive(Debug, Clone, Queryable, Identifiable, Associations, PartialEq)]
#[table_name = "events"]
#[belongs_to(User, foreign_key = "created_by")]
pub struct Event {
    pub id: EventId,
    pub id_serial: EventSerialId,
    pub title: String,
    pub description: String,
    pub room: RoomId,
    pub created_by: UserId,
    pub created_at: DateTime<Utc>,
    pub updated_by: UserId,
    pub updated_at: DateTime<Utc>,
    pub is_time_independent: bool,
    pub is_all_day: Option<bool>,

    /// start datetime of the event
    pub starts_at: Option<DateTime<Utc>>,

    /// timezone of the start-datetime of the event
    pub starts_at_tz: Option<TimeZone>,

    /// end datetime of the event
    ///
    /// For recurring events contains the timestamp of the last occurrence
    pub ends_at: Option<DateTime<Utc>>,

    /// timezone of the ends_at datetime
    pub ends_at_tz: Option<TimeZone>,

    /// Only for recurring events, since ends_at contains the information
    /// about the last occurrence of the recurring series this duration value
    /// MUST be used to calculate the event instances length
    pub duration_secs: Option<i32>,

    pub is_recurring: Option<bool>,
    pub recurrence_pattern: Option<String>,
}

impl Event {
    /// Returns the ends_at value of the first occurrence of the event
    pub fn ends_at_of_first_occurrence(&self) -> Option<(DateTime<Utc>, TimeZone)> {
        if self.is_recurring.unwrap_or_default() {
            // Recurring events have the last occurrence of the recurrence saved in the ends_at fields
            // So we get the starts_at_dt and add the duration_secs field to it
            if let (Some(starts_at_dt), Some(dur), Some(tz)) =
                (self.starts_at, self.duration_secs, self.ends_at_tz)
            {
                Some((starts_at_dt + chrono::Duration::seconds(i64::from(dur)), tz))
            } else {
                None
            }
        } else if let (Some(dt), Some(tz)) = (self.ends_at, self.ends_at_tz) {
            // Non recurring events just directly use the ends_at field from the db
            Some((dt, tz))
        } else {
            None
        }
    }
}

impl HasUsers for &Event {
    fn populate(self, dst: &mut Vec<UserId>) {
        dst.push(self.created_by);
        dst.push(self.updated_by);
    }
}

pub struct GetEventsCursor {
    pub from_id: EventId,
    pub from_created_at: DateTime<Utc>,
    pub from_starts_at: Option<DateTime<Utc>>,
}

impl GetEventsCursor {
    pub fn from_last_event_in_query(event: &Event) -> Self {
        Self {
            from_id: event.id,
            from_created_at: event.created_at,
            from_starts_at: event.starts_at,
        }
    }
}

impl Event {
    #[tracing::instrument(err, skip_all)]
    pub fn get(conn: &DbConnection, event_id: EventId) -> Result<Event> {
        let query = events::table.filter(events::id.eq(event_id));

        let event = query.first(conn)?;

        Ok(event)
    }

    #[tracing::instrument(err, skip_all)]
    #[allow(clippy::type_complexity)]
    pub fn get_with_invite_and_room(
        conn: &DbConnection,
        user_id: UserId,
        event_id: EventId,
    ) -> Result<(Event, Option<EventInvite>, Room, Option<SipConfig>, bool)> {
        let query = events::table
            .left_join(
                event_invites::table.on(event_invites::event_id
                    .eq(events::id)
                    .and(event_invites::invitee.eq(user_id))),
            )
            .left_join(
                event_favorites::table.on(event_favorites::event_id
                    .eq(events::id)
                    .and(event_favorites::user_id.eq(user_id))),
            )
            .inner_join(rooms::table.on(events::room.eq(rooms::id)))
            .left_join(sip_configs::table.on(rooms::id.eq(sip_configs::room)))
            .select((
                events::all_columns,
                event_invites::all_columns.nullable(),
                rooms::all_columns,
                sip_configs::all_columns.nullable(),
                event_favorites::user_id.is_not_null(),
            ))
            .filter(events::id.eq(event_id));

        let (event, invite, room, sip_config, is_favorite) = query.first(conn)?;

        Ok((event, invite, room, sip_config, is_favorite))
    }

    #[tracing::instrument(err, skip_all)]
    #[allow(clippy::type_complexity)]
    pub fn get_with_room(
        conn: &DbConnection,
        event_id: EventId,
    ) -> Result<(Event, Room, Option<SipConfig>)> {
        let query = events::table
            .inner_join(rooms::table.on(events::room.eq(rooms::id)))
            .left_join(sip_configs::table.on(rooms::id.eq(sip_configs::room)))
            .select((
                events::all_columns,
                rooms::all_columns,
                sip_configs::all_columns.nullable(),
            ))
            .filter(events::id.eq(event_id));

        let (event, room, sip_config) = query.first(conn)?;

        Ok((event, room, sip_config))
    }

    #[tracing::instrument(err, skip_all)]
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    pub fn get_all_for_user_paginated(
        conn: &DbConnection,
        user_id: UserId,
        only_favorites: bool,
        invite_status_filter: Vec<EventInviteStatus>,
        time_min: Option<DateTime<Utc>>,
        time_max: Option<DateTime<Utc>>,
        cursor: Option<GetEventsCursor>,
        limit: i64,
    ) -> Result<
        Vec<(
            Event,
            Option<EventInvite>,
            Room,
            Option<SipConfig>,
            Vec<EventException>,
            bool,
        )>,
    > {
        // Filter applied to all events which validates that the event is either created by
        // the given user or a invite to the event exists for the user
        let event_related_to_user_id = events::created_by
            .eq(user_id)
            .or(event_invites::invitee.eq(user_id));

        // Create query which select events and joins into the room of the event
        let mut query = events::table
            .left_join(
                event_invites::table.on(event_invites::event_id
                    .eq(events::id)
                    .and(event_invites::invitee.eq(user_id))),
            )
            .left_join(
                event_favorites::table.on(event_favorites::event_id
                    .eq(events::id)
                    .and(event_favorites::user_id.eq(user_id))),
            )
            .inner_join(rooms::table)
            .left_join(sip_configs::table.on(rooms::id.eq(sip_configs::room)))
            .select((
                events::all_columns,
                event_invites::all_columns.nullable(),
                rooms::all_columns,
                sip_configs::all_columns.nullable(),
                event_favorites::user_id.is_not_null(),
            ))
            .filter(event_related_to_user_id)
            .order_by(events::starts_at.asc().nulls_first())
            .then_order_by(events::created_at.asc())
            .then_order_by(events::id)
            .limit(limit)
            .into_boxed::<Pg>();

        // TODO(r.floren): Write tests for this cursor behavior

        // Tuples/Composite types are ordered by lexical ordering
        if let Some(cursor) = cursor {
            if let Some(from_starts_at) = cursor.from_starts_at {
                let expr =
                    AsExpression::<Record<(Nullable<Timestamptz>,Timestamptz, Uuid)>>::as_expression((
                        events::starts_at,
                        events::created_at,
                        events::id
                    ));

                query =
                    query.filter(expr.gt((from_starts_at, cursor.from_created_at, cursor.from_id)));
            } else {
                let expr = AsExpression::<Record<(Timestamptz, Uuid)>>::as_expression((
                    events::created_at,
                    events::id,
                ));

                query = query.filter(expr.gt((cursor.from_created_at, cursor.from_id)));
            }
        }

        // Add filters to query depending on the time_(min/max) parameters
        match (time_min, time_max) {
            (Some(time_min), Some(time_max)) => {
                query = query.filter(
                    events::starts_at
                        .ge(time_min)
                        .and(events::ends_at.le(time_max)),
                );
            }
            (Some(time_min), None) => {
                query = query.filter(events::starts_at.ge(time_min));
            }
            (None, Some(time_max)) => {
                query = query.filter(events::ends_at.le(time_max));
            }
            (None, None) => {
                // no filters to apply
            }
        }

        if only_favorites {
            query = query.filter(event_favorites::user_id.is_not_null());
        }

        if !invite_status_filter.is_empty() {
            if invite_status_filter.contains(&EventInviteStatus::Accepted) {
                // edge case to allow event creators to filter created events by 'accepted'
                query = query.filter(
                    event_invites::status
                        .eq_any(invite_status_filter)
                        .or(event_invites::status.is_null()),
                );
            } else {
                query = query.filter(event_invites::status.eq_any(invite_status_filter));
            }
        }

        let events_with_invite_and_room: Vec<(
            Event,
            Option<EventInvite>,
            Room,
            Option<SipConfig>,
            bool,
        )> = query.load(conn)?;

        let mut events_with_invite_room_and_exceptions =
            Vec::with_capacity(events_with_invite_and_room.len());

        for (event, invite, room, sip_config, is_favorite) in events_with_invite_and_room {
            let exceptions = if event.is_recurring.unwrap_or_default() {
                event_exceptions::table
                    .filter(event_exceptions::event_id.eq(event.id))
                    .load(conn)?
            } else {
                vec![]
            };

            events_with_invite_room_and_exceptions.push((
                event,
                invite,
                room,
                sip_config,
                exceptions,
                is_favorite,
            ));
        }

        Ok(events_with_invite_room_and_exceptions)
    }

    #[tracing::instrument(err, skip_all)]
    pub fn delete_by_id(conn: &DbConnection, event_id: EventId) -> Result<()> {
        diesel::delete(events::table)
            .filter(events::id.eq(event_id))
            .execute(conn)?;

        Ok(())
    }

    /// Returns all [`Event`]s in the given [`RoomId`].
    ///
    /// This is needed because when rescheduling an Event from time x onwards, we create a new Event and both reference the room.
    #[tracing::instrument(err, skip_all)]
    pub fn get_all_ids_for_room(conn: &DbConnection, room_id: RoomId) -> Result<Vec<EventId>> {
        let query = events::table
            .select(events::id)
            .filter(events::room.eq(room_id));

        let events = query.load(conn)?;

        Ok(events)
    }

    /// Deletes all [`Event`]s in a given [`RoomId`]
    ///
    /// Fastpath for deleting multiple events in room
    #[tracing::instrument(err, skip_all)]
    pub fn delete_all_for_room(conn: &DbConnection, room_id: RoomId) -> Result<()> {
        diesel::delete(events::table)
            .filter(events::room.eq(room_id))
            .execute(conn)?;

        Ok(())
    }
}

#[derive(Debug, Insertable)]
#[table_name = "events"]
pub struct NewEvent {
    pub title: String,
    pub description: String,
    pub room: RoomId,
    pub created_by: UserId,
    pub updated_by: UserId,
    pub is_time_independent: bool,
    pub is_all_day: Option<bool>,
    pub starts_at: Option<DateTime<Tz>>,
    pub starts_at_tz: Option<TimeZone>,
    pub ends_at: Option<DateTime<Tz>>,
    pub ends_at_tz: Option<TimeZone>,
    pub duration_secs: Option<i32>,
    pub is_recurring: Option<bool>,
    pub recurrence_pattern: Option<String>,
}

impl NewEvent {
    #[tracing::instrument(err, skip_all)]
    pub fn insert(self, conn: &DbConnection) -> Result<Event> {
        let query = self.insert_into(events::table);

        let event = query.get_result(conn)?;

        Ok(event)
    }
}

#[derive(Debug, AsChangeset)]
#[table_name = "events"]
pub struct UpdateEvent {
    pub title: Option<String>,
    pub description: Option<String>,
    pub updated_by: UserId,
    pub updated_at: DateTime<Utc>,
    pub is_time_independent: Option<bool>,
    pub is_all_day: Option<Option<bool>>,
    pub starts_at: Option<Option<DateTime<Tz>>>,
    pub starts_at_tz: Option<Option<TimeZone>>,
    pub ends_at: Option<Option<DateTime<Tz>>>,
    pub ends_at_tz: Option<Option<TimeZone>>,
    pub duration_secs: Option<Option<i32>>,
    pub is_recurring: Option<Option<bool>>,
    pub recurrence_pattern: Option<Option<String>>,
}

impl UpdateEvent {
    #[tracing::instrument(err, skip_all)]
    pub fn apply(self, conn: &DbConnection, event_id: EventId) -> Result<Event> {
        let query = diesel::update(events::table)
            .filter(events::id.eq(event_id))
            .set(self)
            .returning(events::all_columns);

        let event = query.get_result(conn)?;

        Ok(event)
    }
}

sql_enum!(
    EventExceptionKind,
    "event_exception_kind",
    EventExceptionKindType,
    "EventExceptionKindType",
    {
        Modified = b"modified",
        Cancelled = b"cancelled",
    }
);

#[derive(Debug, Queryable, Identifiable, Associations)]
#[table_name = "event_exceptions"]
#[belongs_to(Event, foreign_key = "event_id")]
#[belongs_to(User, foreign_key = "created_by")]
pub struct EventException {
    pub id: EventExceptionId,
    pub event_id: EventId,
    pub exception_date: DateTime<Utc>,
    pub exception_date_tz: TimeZone,
    pub created_by: UserId,
    pub created_at: DateTime<Utc>,
    pub kind: EventExceptionKind,
    pub title: Option<String>,
    pub description: Option<String>,
    pub is_all_day: Option<bool>,
    pub starts_at: Option<DateTime<Utc>>,
    pub starts_at_tz: Option<TimeZone>,
    pub ends_at: Option<DateTime<Utc>>,
    pub ends_at_tz: Option<TimeZone>,
}

impl HasUsers for &EventException {
    fn populate(self, dst: &mut Vec<UserId>) {
        dst.push(self.created_by);
    }
}

impl EventException {
    #[tracing::instrument(err, skip_all)]
    pub fn get_for_event(
        conn: &DbConnection,
        event_id: EventId,
        datetime: DateTime<Utc>,
    ) -> Result<Option<EventException>> {
        let query = event_exceptions::table.filter(
            event_exceptions::event_id
                .eq(event_id)
                .and(event_exceptions::exception_date.eq(datetime)),
        );

        let exceptions = query.first(conn).optional()?;

        Ok(exceptions)
    }

    #[tracing::instrument(err, skip_all)]
    pub fn get_all_for_event(
        conn: &DbConnection,
        event_id: EventId,
        datetimes: &[DateTime<Utc>],
    ) -> Result<Vec<EventException>> {
        let query = event_exceptions::table.filter(
            event_exceptions::event_id
                .eq(event_id)
                .and(event_exceptions::exception_date.eq_any(datetimes)),
        );

        let exceptions = query.load(conn).optional()?.unwrap_or_default();

        Ok(exceptions)
    }

    #[tracing::instrument(err, skip_all)]
    pub fn delete_all_for_event(conn: &DbConnection, event_id: EventId) -> Result<()> {
        let query =
            diesel::delete(event_exceptions::table).filter(event_exceptions::event_id.eq(event_id));

        query.execute(conn)?;

        Ok(())
    }
}

#[derive(Debug, Insertable)]
#[table_name = "event_exceptions"]
pub struct NewEventException {
    pub event_id: EventId,
    pub exception_date: DateTime<Utc>,
    pub exception_date_tz: TimeZone,
    pub created_by: UserId,
    pub kind: EventExceptionKind,
    pub title: Option<String>,
    pub description: Option<String>,
    pub is_all_day: Option<bool>,
    pub starts_at: Option<DateTime<Tz>>,
    pub starts_at_tz: Option<TimeZone>,
    pub ends_at: Option<DateTime<Tz>>,
    pub ends_at_tz: Option<TimeZone>,
}

impl NewEventException {
    #[tracing::instrument(err, skip_all)]
    pub fn insert(self, conn: &DbConnection) -> Result<EventException> {
        let query = self.insert_into(event_exceptions::table);

        let event_exception = query.get_result(conn)?;

        Ok(event_exception)
    }
}

#[derive(Debug, AsChangeset)]
#[table_name = "event_exceptions"]
pub struct UpdateEventException {
    pub kind: Option<EventExceptionKind>,
    pub title: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub is_all_day: Option<Option<bool>>,
    pub starts_at: Option<Option<DateTime<Tz>>>,
    pub starts_at_tz: Option<Option<TimeZone>>,
    pub ends_at: Option<Option<DateTime<Tz>>>,
    pub ends_at_tz: Option<Option<TimeZone>>,
}

impl UpdateEventException {
    #[tracing::instrument(err, skip_all)]
    pub fn apply(
        self,
        conn: &DbConnection,
        event_exception_id: EventExceptionId,
    ) -> Result<EventException> {
        let query = diesel::update(event_exceptions::table)
            .filter(event_exceptions::id.eq(event_exception_id))
            .set(self)
            .returning(event_exceptions::all_columns);

        let exception = query.get_result(conn)?;

        Ok(exception)
    }
}

sql_enum!(
    #[derive(Serialize, PartialEq)]
    #[serde(rename_all = "snake_case")]
    EventInviteStatus,
    "event_invite_status",
    EventInviteStatusType,
    "EventInviteStatusType",
    {
        Pending = b"pending",
        Accepted = b"accepted",
        Tentative = b"tentative",
        Declined = b"declined",
    }
);

impl FromStr for EventInviteStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "accepted" => Ok(Self::Accepted),
            "tentative" => Ok(Self::Tentative),
            "declined" => Ok(Self::Declined),
            _ => Err(format!("unknown invite_status {s:?}")),
        }
    }
}

#[derive(Debug, Queryable, Identifiable, Associations)]
#[table_name = "event_invites"]
#[belongs_to(Event, foreign_key = "event_id")]
#[belongs_to(User, foreign_key = "invitee")]
pub struct EventInvite {
    pub id: EventInviteId,
    pub event_id: EventId,
    pub invitee: UserId,
    pub created_by: UserId,
    pub created_at: DateTime<Utc>,
    pub status: EventInviteStatus,
}

impl EventInvite {
    #[tracing::instrument(err, skip_all)]
    pub fn get_for_events(
        conn: &DbConnection,
        events: &[&Event],
    ) -> Result<Vec<Vec<(EventInvite, User)>>> {
        conn.transaction(|| {
            let invites: Vec<EventInvite> = EventInvite::belonging_to(events).load(conn)?;
            let mut user_ids: Vec<UserId> = invites.iter().map(|x| x.invitee).collect();
            // Small optimization to filter out duplicates
            user_ids.sort_unstable();
            user_ids.dedup();

            let users = User::get_all_by_ids(conn, &user_ids)?;

            let invites_by_event: Vec<Vec<EventInvite>> = invites.grouped_by(events);
            let mut invites_with_users_by_event = Vec::with_capacity(events.len());

            for invites in invites_by_event {
                let mut invites_with_users = Vec::with_capacity(invites.len());

                for invite in invites {
                    let user = users
                        .iter()
                        .find(|user| user.id == invite.invitee)
                        .ok_or_else(|| {
                            DatabaseError::Custom("bug: user invite invitee missing".into())
                        })?;

                    invites_with_users.push((invite, user.clone()))
                }

                invites_with_users_by_event.push(invites_with_users);
            }

            Ok(invites_with_users_by_event)
        })
    }

    #[tracing::instrument(err, skip_all)]
    pub fn get_for_event_paginated(
        conn: &DbConnection,
        event_id: EventId,
        limit: i64,
        page: i64,
    ) -> Result<(Vec<(EventInvite, User)>, i64)> {
        let query = event_invites::table
            .inner_join(users::table.on(event_invites::invitee.eq(users::id)))
            .filter(event_invites::columns::event_id.eq(event_id))
            .order(event_invites::created_at.desc())
            .then_order_by(event_invites::created_by.desc())
            .then_order_by(event_invites::invitee.desc())
            .paginate_by(limit, page);
        let invites = query.load_and_count(conn)?;

        Ok(invites)
    }

    #[tracing::instrument(err, skip_all)]
    pub fn get_pending_for_user(conn: &DbConnection, user_id: UserId) -> Result<Vec<EventInvite>> {
        let query = event_invites::table.filter(
            event_invites::invitee
                .eq(user_id)
                .and(event_invites::status.eq(EventInviteStatus::Pending)),
        );

        let event_invites = query.load(conn)?;

        Ok(event_invites)
    }

    #[tracing::instrument(err, skip_all)]
    pub fn delete_by_invitee(
        conn: &DbConnection,
        event_id: EventId,
        invitee: UserId,
    ) -> Result<EventInvite> {
        let query = diesel::delete(event_invites::table)
            .filter(
                event_invites::event_id
                    .eq(event_id)
                    .and(event_invites::invitee.eq(invitee)),
            )
            .returning(event_invites::all_columns);

        let event_invite = query.get_result(conn)?;

        Ok(event_invite)
    }
}

#[derive(Insertable)]
#[table_name = "event_invites"]
pub struct NewEventInvite {
    pub event_id: EventId,
    pub invitee: UserId,
    pub created_by: UserId,
    pub created_at: Option<DateTime<Utc>>,
}

impl NewEventInvite {
    /// Tries to insert the EventInvite into the database
    ///
    /// When yielding a unique key violation, None is returned.
    #[tracing::instrument(err, skip_all)]
    pub fn try_insert(self, conn: &DbConnection) -> Result<Option<EventInvite>> {
        let query = self.insert_into(event_invites::table);

        let result = query.get_result(conn);

        match result {
            Ok(event_invite) => Ok(Some(event_invite)),
            Err(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                ..,
            )) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[derive(AsChangeset)]
#[table_name = "event_invites"]
pub struct UpdateEventInvite {
    pub status: EventInviteStatus,
}

impl UpdateEventInvite {
    /// Apply the update to the invite where `user_id` is the invitee
    #[tracing::instrument(err, skip_all)]
    pub fn apply(
        self,
        conn: &DbConnection,
        user_id: UserId,
        event_id: EventId,
    ) -> Result<EventInvite> {
        // TODO: Check if the update actually applied a change
        // Use something like
        // UPDATE event_invites SET status = $status WHERE id = $id RETURNING id, status, (SELECT status FROM tmp WHERE id = $id);
        // or
        // UPDATE event_invites SET status = $status WHERE id = $id FROM event_invites old RETURNING old.*;
        // and compare the value to the set one to return if the value was changed
        let query = diesel::update(event_invites::table)
            .filter(
                event_invites::event_id
                    .eq(event_id)
                    .and(event_invites::invitee.eq(user_id)),
            )
            .set(self)
            // change it here
            .returning(event_invites::all_columns);

        let event_invite = query.get_result(conn)?;

        Ok(event_invite)
    }
}

#[derive(Associations, Identifiable, Queryable)]
#[table_name = "event_favorites"]
#[primary_key(user_id, event_id)]
#[belongs_to(User)]
#[belongs_to(Event)]
pub struct EventFavorite {
    pub user_id: UserId,
    pub event_id: EventId,
}

impl EventFavorite {
    /// Deletes a EventFavorite entry by user_id and event_id
    ///
    /// Returns true if something was deleted
    #[tracing::instrument(err, skip_all)]
    pub fn delete_by_id(conn: &DbConnection, user_id: UserId, event_id: EventId) -> Result<bool> {
        let lines_changes = diesel::delete(event_favorites::table)
            .filter(
                event_favorites::user_id
                    .eq(user_id)
                    .and(event_favorites::event_id.eq(event_id)),
            )
            .execute(conn)?;

        Ok(lines_changes > 0)
    }
}

#[derive(Insertable)]
#[table_name = "event_favorites"]
pub struct NewEventFavorite {
    pub user_id: UserId,
    pub event_id: EventId,
}

impl NewEventFavorite {
    /// Tries to insert the NewEventFavorite into the database
    ///
    /// When yielding a unique key violation, None is returned.
    #[tracing::instrument(err, skip_all)]
    pub fn try_insert(self, conn: &DbConnection) -> Result<Option<EventFavorite>> {
        let query = self.insert_into(event_favorites::table);

        let result = query.get_result(conn);

        match result {
            Ok(event_favorite) => Ok(Some(event_favorite)),
            Err(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                ..,
            )) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

// Below impls allow for usage of diesel's BelongsTo traits on &[&Event] to avoid
// cloning the events into a array just for the EventInvites::get_for_events
impl BelongsTo<&Event> for EventInvite {
    type ForeignKey = EventId;

    type ForeignKeyColumn = event_invites::event_id;

    fn foreign_key(&self) -> Option<&Self::ForeignKey> {
        Some(&self.event_id)
    }

    fn foreign_key_column() -> Self::ForeignKeyColumn {
        event_invites::event_id
    }
}

impl Identifiable for &&Event {
    type Id = EventId;

    fn id(self) -> Self::Id {
        self.id
    }
}
