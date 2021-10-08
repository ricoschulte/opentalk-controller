use super::{DatabaseError, Paginate, Result};
use crate::db::rooms::{Room, RoomId};
use crate::db::schema::invites;
use crate::db::users::{User, UserId};
use crate::db::DbInterface;
use crate::{impl_from_redis_value_de, impl_to_redis_args_se};
use diesel::result::{DatabaseErrorKind, Error};
use diesel::{
    BelongingToDsl, ExpressionMethods, Identifiable, QueryDsl, QueryResult, Queryable, RunQueryDsl,
};
use serde::Serialize;

diesel_newtype!(InviteCodeUuid(uuid::Uuid) => diesel::sql_types::Uuid, "diesel::sql_types::Uuid");

/// Diesel invites struct
///
/// Represents an invite in the database
#[derive(Debug, Queryable, Identifiable, Associations)]
#[belongs_to(User, foreign_key = "created_by")]
pub struct Invite {
    pub id: i64,
    pub uuid: InviteCodeUuid,
    pub created: chrono::DateTime<chrono::Utc>,
    pub created_by: UserId,
    pub updated: chrono::DateTime<chrono::Utc>,
    pub updated_by: UserId,
    pub room: RoomId,
    pub active: bool,
    pub expiration: Option<chrono::DateTime<chrono::Utc>>,
}

/// Diesel invites struct
///
/// Represents a new invite in the database
#[derive(Debug, Clone, Insertable)]
#[table_name = "invites"]
pub struct NewInvite<'a> {
    pub uuid: &'a InviteCodeUuid,
    pub created: &'a chrono::DateTime<chrono::Utc>,
    pub created_by: &'a UserId,
    pub updated: &'a chrono::DateTime<chrono::Utc>,
    pub updated_by: &'a UserId,
    pub room: &'a RoomId,
    pub active: bool,
    pub expiration: Option<&'a chrono::DateTime<chrono::Utc>>,
}

/// Diesel invites struct
///
/// Represents a changeset of in invite
#[derive(Debug, AsChangeset)]
#[table_name = "invites"]
pub struct UpdateInvite<'a> {
    pub updated: Option<&'a chrono::DateTime<chrono::Utc>>,
    pub updated_by: Option<&'a UserId>,
    pub room: Option<&'a RoomId>,
    pub active: Option<bool>,
    pub expiration: Option<Option<&'a chrono::DateTime<chrono::Utc>>>,
}

impl DbInterface {
    #[tracing::instrument(skip(self, new_invite))]
    pub fn new_invite(&self, new_invite: NewInvite) -> Result<Invite> {
        let conn = self.get_con()?;

        // a UUID collision will result in an internal server error
        let invite_result: QueryResult<Invite> = diesel::insert_into(invites::table)
            .values(new_invite)
            .get_result(&conn);

        match invite_result {
            Ok(invite) => Ok(invite),
            Err(e) => {
                log::error!("Query error creating new room, {}", e);
                Err(e.into())
            }
        }
    }
    #[tracing::instrument(skip(self))]
    pub fn get_invites_for_room_paginated(
        &self,
        room: &Room,
        limit: i64,
        page: i64,
    ) -> Result<(Vec<Invite>, i64)> {
        let conn = self.get_con()?;

        let query = invites::table
            .filter(invites::room.eq(room.uuid))
            .order(invites::updated.desc())
            .paginate_by(limit, page);
        let query_result = query.load_and_count::<Invite, _>(&conn);

        match query_result {
            Ok(result) => Ok(result),
            Err(e) => {
                log::error!("Query error getting owned rooms, {}", e);
                Err(e.into())
            }
        }
    }
    #[tracing::instrument(skip(self))]
    pub fn get_invite(&self, invite_code: &InviteCodeUuid) -> Result<Invite> {
        let conn = self.get_con()?;

        let query = invites::table
            .filter(invites::uuid.eq(invite_code))
            .order(invites::updated.desc());
        query.first(&conn).map_err(Into::into)
    }

    #[tracing::instrument(skip(self))]
    pub fn update_invite(
        &self,
        invite_code: &InviteCodeUuid,
        changeset: &UpdateInvite,
    ) -> Result<Invite> {
        let conn = self.get_con()?;

        let query = diesel::update(invites::table)
            .filter(invites::uuid.eq(invite_code))
            .set(changeset)
            .returning(invites::all_columns);
        query.get_result(&conn).map_err(Into::into)
    }

    #[tracing::instrument(skip(self))]
    pub fn deactivate_invite(&self, invite_code: &InviteCodeUuid) -> Result<Invite> {
        let conn = self.get_con()?;

        let query = diesel::update(invites::table)
            .filter(invites::uuid.eq(invite_code))
            .set(&UpdateInvite {
                active: Some(false),
                updated: None,
                updated_by: None,
                room: None,
                expiration: None,
            });
        query.get_result(&conn).map_err(Into::into)
    }
}
