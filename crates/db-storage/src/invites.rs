use crate::rooms::RoomId;
use crate::schema::{invites, users};
use crate::users::{SerialUserId, User};
use chrono::{DateTime, Utc};
use database::{DbInterface, Paginate, Result};
use diesel::dsl::any;
use diesel::{ExpressionMethods, Identifiable, JoinOnDsl, QueryDsl, Queryable, RunQueryDsl};
use std::collections::{HashMap, HashSet};

diesel_newtype! {
    #[derive(Copy)] InviteCodeId(uuid::Uuid) => diesel::sql_types::Uuid, "diesel::sql_types::Uuid", "/invites/"
}

/// Diesel invites struct
///
/// Represents an invite in the database
#[derive(Debug, Queryable, Identifiable, Associations)]
#[belongs_to(User, foreign_key = "created_by")]
pub struct Invite {
    pub id: i64,
    pub uuid: InviteCodeId,
    pub created: DateTime<Utc>,
    pub created_by: SerialUserId,
    pub updated: DateTime<Utc>,
    pub updated_by: SerialUserId,
    pub room: RoomId,
    pub active: bool,
    pub expiration: Option<DateTime<Utc>>,
}

/// Diesel invites struct
///
/// Represents a new invite in the database
#[derive(Debug, Clone, Insertable)]
#[table_name = "invites"]
pub struct NewInvite<'a> {
    pub uuid: &'a InviteCodeId,
    pub created: &'a DateTime<Utc>,
    pub created_by: &'a SerialUserId,
    pub updated: &'a DateTime<Utc>,
    pub updated_by: &'a SerialUserId,
    pub room: &'a RoomId,
    pub active: bool,
    pub expiration: Option<&'a DateTime<Utc>>,
}

/// Diesel invites struct
///
/// Represents a changeset of in invite
#[derive(Debug, AsChangeset)]
#[table_name = "invites"]
pub struct UpdateInvite<'a> {
    pub updated: Option<&'a DateTime<Utc>>,
    pub updated_by: Option<&'a SerialUserId>,
    pub room: Option<&'a RoomId>,
    pub active: Option<bool>,
    pub expiration: Option<Option<&'a DateTime<Utc>>>,
}

pub type InviteWithUsers = (Invite, User, User);

pub trait DbInvitesEx: DbInterface {
    #[tracing::instrument(err, skip_all)]
    fn new_invite(&self, new_invite: NewInvite) -> Result<Invite> {
        let conn = self.get_conn()?;

        // a UUID collision will result in an internal server error
        let invite = diesel::insert_into(invites::table)
            .values(new_invite)
            .get_result(&conn)?;

        Ok(invite)
    }

    /// Created a new invite
    ///
    /// Returns:
    /// (Invite, CreatedByUser, UpdatedByUser) - The created invite along with the users that created and updated the invite
    #[tracing::instrument(err, skip_all)]
    fn new_invite_with_users(&self, new_invite: NewInvite) -> Result<InviteWithUsers> {
        let conn = self.get_conn()?;

        // a UUID collision will result in an internal server error
        let invite = diesel::insert_into(invites::table)
            .values(new_invite)
            .get_result::<Invite>(&conn)?;
        let created_by = users::dsl::users
            .find(invite.created_by)
            .get_result::<User>(&conn)?;
        let updated_by = users::dsl::users
            .find(invite.updated_by)
            .get_result::<User>(&conn)?;

        Ok((invite, created_by, updated_by))
    }

    /// Returns a paginated view on invites for the given room
    ///
    ///
    /// Returns:
    /// Vec<(Invite, CreatedByUser, UpdatedByUser)> - A Vec of invites along with the users that created and updated the invite
    #[tracing::instrument(err, skip_all, fields(%limit, %page))]
    fn get_invites_for_room_paginated(
        &self,
        room_id: RoomId,
        limit: i64,
        page: i64,
    ) -> Result<(Vec<Invite>, i64)> {
        let conn = self.get_conn()?;

        let query = invites::table
            .filter(invites::room.eq(room_id))
            .order(invites::updated.desc())
            .paginate_by(limit, page);
        let invites_with_total = query.load_and_count::<Invite, _>(&conn)?;

        Ok(invites_with_total)
    }

    /// Returns a paginated view on invites for the given room
    ///
    /// Returns:
    /// Vec<(Invite, CreatedByUser, UpdatedByUser)> - A Vec of invites along with the users that created and updated the invite
    #[tracing::instrument(err, skip_all, fields(%limit, %page))]
    fn get_invites_for_room_with_users_paginated(
        &self,
        room_id: RoomId,
        limit: i64,
        page: i64,
    ) -> Result<(Vec<InviteWithUsers>, i64)> {
        let conn = self.get_conn()?;

        let query = invites::table
            .filter(invites::room.eq(room_id))
            .inner_join(users::table.on(invites::created_by.eq(users::id)))
            .order(invites::updated.desc())
            .paginate_by(limit, page);
        let (query_result, total) = query.load_and_count::<(Invite, User), _>(&conn)?;

        // This needs urgent improvement, this will come up more times when we follow the created_by, updated_by pattern.
        let users_set = query_result
            .iter()
            .fold(HashSet::new(), |mut acc, (user, _)| {
                acc.insert(user.updated_by);
                acc
            });
        let users = users_set.iter().collect::<Vec<_>>();

        let query = users::table.filter(users::id.eq(any(users)));
        let updated_by = query.get_results::<User>(&conn)?;
        let updated_by = updated_by
            .into_iter()
            .map(|u| (u.id, u))
            .collect::<HashMap<_, _>>();

        Ok((
            query_result
                .into_iter()
                .map(|(invite, created_by)| {
                    let updated_by_id = invite.updated_by;
                    (
                        invite,
                        created_by,
                        updated_by
                            .get(&updated_by_id)
                            .expect("Some Foreign Key was wrong in our database")
                            .clone(),
                    )
                })
                .collect::<Vec<_>>(),
            total,
        ))
    }

    /// Returns a paginated view on invites for the given room
    ///
    /// Filters based on the passed user. Only invites are returned that where created or updated by the passed in user.
    ///
    /// Returns:
    /// Vec<(Invite, CreatedByUser, UpdatedByUser)> - A Vec of invites along with the users that created and updated the invite
    #[tracing::instrument(err, skip_all, fields(%limit, %page))]
    fn get_invites_for_room_with_users_by_ids_paginated(
        &self,
        room_id: RoomId,
        ids: &[InviteCodeId],
        limit: i64,
        page: i64,
    ) -> Result<(Vec<InviteWithUsers>, i64)> {
        let conn = self.get_conn()?;

        let query = invites::table
            .filter(invites::room.eq(room_id))
            .filter(invites::uuid.eq(any(ids)))
            .inner_join(users::table.on(invites::created_by.eq(users::id)))
            .order(invites::updated.desc())
            .paginate_by(limit, page);
        let (query_result, total) = query.load_and_count::<(Invite, User), _>(&conn)?;

        // This needs urgent improvement, this will come up more times when we follow the created_by, updated_by pattern.
        let users_set = query_result
            .iter()
            .fold(HashSet::new(), |mut acc, (user, _)| {
                acc.insert(user.updated_by);
                acc
            });
        let users = users_set.iter().collect::<Vec<_>>();

        let query = users::table.filter(users::id.eq(any(users)));
        let updated_by = query.get_results::<User>(&conn)?;
        let updated_by = updated_by
            .into_iter()
            .map(|u| (u.id, u))
            .collect::<HashMap<_, _>>();

        Ok((
            query_result
                .into_iter()
                .map(|(invite, created_by)| {
                    let updated_by_id = invite.updated_by;
                    (
                        invite,
                        created_by,
                        updated_by
                            .get(&updated_by_id)
                            .expect("Some Foreign Key was wrong in our database")
                            .clone(),
                    )
                })
                .collect::<Vec<_>>(),
            total,
        ))
    }

    #[tracing::instrument(err, skip_all)]
    fn get_invite(&self, invite_code_id: InviteCodeId) -> Result<Invite> {
        let conn = self.get_conn()?;

        let query = invites::table
            .filter(invites::uuid.eq(invite_code_id))
            .order(invites::updated.desc());
        query.first(&conn).map_err(Into::into)
    }

    #[tracing::instrument(err, skip_all)]
    fn get_invite_with_users(&self, invite_code_id: InviteCodeId) -> Result<InviteWithUsers> {
        // Diesel currently does not support joining a table twice, so we need to join once and do a second select.
        // Or we need to write our handwritten SQL here.
        let conn = self.get_conn()?;

        let query = invites::table
            .filter(invites::uuid.eq(invite_code_id))
            .inner_join(users::table.on(invites::created_by.eq(users::id)))
            .order(invites::updated.desc());
        let (invite, created_by) = query.first::<(Invite, User)>(&conn)?;
        let query = users::table.filter(users::id.eq(invite.updated_by));
        Ok((invite, created_by, query.first(&conn)?))
    }

    #[tracing::instrument(err, skip_all)]
    fn update_invite(
        &self,
        invite_code_id: InviteCodeId,
        changeset: &UpdateInvite,
    ) -> Result<Invite> {
        let conn = self.get_conn()?;

        let query = diesel::update(invites::table)
            .filter(invites::uuid.eq(invite_code_id))
            .set(changeset)
            .returning(invites::all_columns);
        query.get_result(&conn).map_err(Into::into)
    }

    #[tracing::instrument(err, skip_all)]
    fn update_invite_with_users(
        &self,
        invite_code_id: InviteCodeId,
        changeset: &UpdateInvite,
    ) -> Result<InviteWithUsers> {
        let conn = self.get_conn()?;

        let query = diesel::update(invites::table)
            .filter(invites::uuid.eq(invite_code_id))
            .set(changeset)
            .returning(invites::all_columns);

        let result = query.get_result::<Invite>(&conn)?;
        let created_by = users::dsl::users
            .find(result.created_by)
            .get_result::<User>(&conn)?;
        let updated_by = users::dsl::users
            .find(result.updated_by)
            .get_result::<User>(&conn)?;
        Ok((result, created_by, updated_by))
    }

    #[tracing::instrument(err, skip_all)]
    fn deactivate_invite(&self, invite_code_id: &InviteCodeId) -> Result<Invite> {
        let conn = self.get_conn()?;

        let query = diesel::update(invites::table)
            .filter(invites::uuid.eq(invite_code_id))
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

impl<T: DbInterface> DbInvitesEx for T {}
