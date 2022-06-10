use super::{Event, EventId, NewEventInvite};
use crate::rooms::RoomId;
use crate::schema::{event_email_invites, event_invites, events};
use crate::users::UserId;
use chrono::{DateTime, Utc};
use database::{DbConnection, Result};
use diesel::prelude::*;
use diesel::{ExpressionMethods, QueryDsl, Queryable, RunQueryDsl};

#[derive(Insertable)]
#[table_name = "event_email_invites"]
pub struct NewEventEmailInvite {
    pub event_id: EventId,
    pub email: String,
    pub created_by: UserId,
}

impl NewEventEmailInvite {
    /// Tries to insert the EventEmailInvite into the database
    ///
    /// When yielding a unique key violation, None is returned.
    #[tracing::instrument(err, skip_all)]
    pub fn try_insert(self, conn: &DbConnection) -> Result<Option<EventEmailInvite>> {
        let query = self.insert_into(event_email_invites::table);

        let result = query.get_result(conn);

        match result {
            Ok(event_email_invites) => Ok(Some(event_email_invites)),
            Err(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                ..,
            )) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[derive(Associations, Identifiable, Queryable)]
#[table_name = "event_email_invites"]
#[primary_key(event_id, email)]
#[belongs_to(Event)]
pub struct EventEmailInvite {
    pub event_id: EventId,
    pub email: String,
    pub created_by: UserId,
    pub created_at: DateTime<Utc>,
}

impl EventEmailInvite {
    pub fn migrate_to_user_invites(
        conn: &DbConnection,
        user_id: UserId,
        email: &str,
    ) -> Result<Vec<(EventId, RoomId)>> {
        conn.transaction(|| {
            let email_invites_with_room: Vec<(EventEmailInvite, RoomId)> =
                event_email_invites::table
                    .filter(event_email_invites::email.eq(email))
                    .inner_join(events::table)
                    .select((event_email_invites::all_columns, events::room))
                    .load(conn)?;

            if email_invites_with_room.is_empty() {
                return Ok(vec![]);
            }

            let event_ids = email_invites_with_room
                .iter()
                .map(|(email_invite, room_id)| (email_invite.event_id, *room_id))
                .collect();

            let new_invites: Vec<_> = email_invites_with_room
                .into_iter()
                .map(|(email_invite, _)| NewEventInvite {
                    event_id: email_invite.event_id,
                    invitee: user_id,
                    created_by: email_invite.created_by,
                    created_at: Some(email_invite.created_at),
                })
                .collect();

            diesel::insert_into(event_invites::table)
                .values(new_invites)
                .on_conflict_do_nothing()
                .execute(conn)?;

            Ok(event_ids)
        })
    }
}
