use crate::rooms::RoomId;
use crate::schema::{invites, users};
use crate::users::{User, UserId};
use chrono::{DateTime, Utc};
use database::{DbConnection, Paginate, Result};
use diesel::{
    BoolExpressionMethods, ExpressionMethods, Identifiable, JoinOnDsl, QueryDsl, Queryable,
    RunQueryDsl,
};
use std::collections::{HashMap, HashSet};

diesel_newtype! {
    #[derive(Copy)] InviteCodeId(uuid::Uuid) => diesel::sql_types::Uuid,
    #[derive(Copy)] InviteCodeSerialId(i64) => diesel::sql_types::BigInt
}

/// Diesel invites struct
///
/// Represents an invite in the database
#[derive(Debug, Queryable, Identifiable, Associations)]
#[diesel(belongs_to(User, foreign_key = created_by))]
pub struct Invite {
    pub id: InviteCodeId,
    pub id_serial: InviteCodeSerialId,
    pub created_by: UserId,
    pub created_at: DateTime<Utc>,
    pub updated_by: UserId,
    pub updated_at: DateTime<Utc>,
    pub room: RoomId,
    pub active: bool,
    pub expiration: Option<DateTime<Utc>>,
}

pub type InviteWithUsers = (Invite, User, User);

impl Invite {
    /// Returns a invites for id
    #[tracing::instrument(err, skip_all)]
    pub fn get(conn: &mut DbConnection, invite_code_id: InviteCodeId) -> Result<Invite> {
        let query = invites::table
            .filter(invites::id.eq(invite_code_id))
            .order(invites::updated_at.desc());

        let invite = query.first(conn)?;

        Ok(invite)
    }

    /// Returns a invites with user metadata for id    
    #[tracing::instrument(err, skip_all)]
    pub fn get_with_users(
        conn: &mut DbConnection,
        invite_code_id: InviteCodeId,
    ) -> Result<InviteWithUsers> {
        // Diesel currently does not support joining a table twice, so we need to join once and do a second select.
        // Or we need to write our handwritten SQL here.
        let query = invites::table
            .filter(invites::id.eq(invite_code_id))
            .inner_join(users::table.on(invites::created_by.eq(users::id)))
            .order(invites::updated_at.desc());
        let (invite, created_by) = query.first::<(Invite, User)>(conn)?;

        let query = users::table.filter(users::id.eq(invite.updated_by));
        Ok((invite, created_by, query.first(conn)?))
    }

    /// Returns a paginated view on invites for the given room
    ///
    ///
    /// Returns:
    /// Vec<(Invite, CreatedByUser, UpdatedByUser)> - A Vec of invites along with the users that created and updated the invite
    #[tracing::instrument(err, skip_all, fields(%limit, %page))]
    pub fn get_all_for_room_paginated(
        conn: &mut DbConnection,
        room_id: RoomId,
        limit: i64,
        page: i64,
    ) -> Result<(Vec<Invite>, i64)> {
        let query = invites::table
            .filter(invites::room.eq(room_id))
            .order(invites::updated_at.desc())
            .paginate_by(limit, page);

        let invites_with_total = query.load_and_count::<Invite, _>(conn)?;

        Ok(invites_with_total)
    }

    /// Returns a paginated view on invites for the given room
    ///
    /// Returns:
    /// Vec<(Invite, CreatedByUser, UpdatedByUser)> - A Vec of invites along with the users that created and updated the invite
    #[tracing::instrument(err, skip_all, fields(%limit, %page))]
    pub fn get_all_for_room_with_users_paginated(
        conn: &mut DbConnection,
        room_id: RoomId,
        limit: i64,
        page: i64,
    ) -> Result<(Vec<InviteWithUsers>, i64)> {
        let query = invites::table
            .filter(invites::room.eq(room_id))
            .inner_join(users::table.on(invites::created_by.eq(users::id)))
            .order(invites::updated_at.desc())
            .paginate_by(limit, page);
        let (invites_with_user, total) = query.load_and_count::<(Invite, User), _>(conn)?;

        // This needs urgent improvement, this will come up more times when we follow the created_by, updated_by pattern.
        let users_set = invites_with_user
            .iter()
            .fold(HashSet::new(), |mut acc, (user, _)| {
                acc.insert(user.updated_by);
                acc
            });

        let users = users_set.iter().collect::<Vec<_>>();

        let query = users::table.filter(users::id.eq_any(users));
        let updated_by = query.get_results::<User>(conn)?;
        let updated_by = updated_by
            .into_iter()
            .map(|u| (u.id, u))
            .collect::<HashMap<_, _>>();

        Ok((
            invites_with_user
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

    pub fn get_first_for_room(
        conn: &mut DbConnection,
        room_id: RoomId,
        user_id: UserId,
    ) -> Result<Invite> {
        let (invites_for_room, _) = Invite::get_all_for_room_paginated(conn, room_id, 1, 1)?;
        let invite_for_room = invites_for_room.into_iter().next();

        let invite_for_room = if let Some(invite) = invite_for_room {
            invite
        } else {
            NewInvite {
                active: true,
                created_by: user_id,
                updated_by: user_id,
                room: room_id,
                expiration: None,
            }
            .insert(conn)?
        };

        Ok(invite_for_room)
    }

    /// Returns a paginated view on invites for the given room
    ///
    /// Filters based on the passed user. Only invites are returned that where created or updated by the passed in user.
    ///
    /// Returns:
    /// Vec<(Invite, CreatedByUser, UpdatedByUser)> - A Vec of invites along with the users that created and updated the invite
    // FIXME(r.floren): When diesel 2.0 gets release this can be reworked to use proper aliases
    #[tracing::instrument(err, skip_all, fields(%limit, %page))]
    pub fn get_all_for_room_with_users_by_ids_paginated(
        conn: &mut DbConnection,
        room_id: RoomId,
        ids: &[InviteCodeId],
        limit: i64,
        page: i64,
    ) -> Result<(Vec<InviteWithUsers>, i64)> {
        let query = invites::table
            .filter(invites::room.eq(room_id))
            .filter(invites::id.eq_any(ids))
            .inner_join(users::table.on(invites::created_by.eq(users::id)))
            .order(invites::updated_at.desc())
            .paginate_by(limit, page);
        let (invites_with_user, total) = query.load_and_count::<(Invite, User), _>(conn)?;

        // This needs urgent improvement, this will come up more times when we follow the created_by, updated_by pattern.
        let users_set = invites_with_user
            .iter()
            .fold(HashSet::new(), |mut acc, (user, _)| {
                acc.insert(user.updated_by);
                acc
            });
        let users = users_set.iter().collect::<Vec<_>>();

        let query = users::table.filter(users::id.eq_any(users));
        let updated_by = query.get_results::<User>(conn)?;
        let updated_by = updated_by
            .into_iter()
            .map(|u| (u.id, u))
            .collect::<HashMap<_, _>>();

        Ok((
            invites_with_user
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
}

/// Diesel invites struct
///
/// Represents a new invite in the database
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = invites)]
pub struct NewInvite {
    pub created_by: UserId,
    pub updated_by: UserId,
    pub room: RoomId,
    pub active: bool,
    pub expiration: Option<DateTime<Utc>>,
}

impl NewInvite {
    #[tracing::instrument(err, skip_all)]
    pub fn insert(self, conn: &mut DbConnection) -> Result<Invite> {
        let query = diesel::insert_into(invites::table).values(self);

        let invite = query.get_result(conn)?;

        Ok(invite)
    }
}

/// Diesel invites struct
///
/// Represents a changeset of in invite
#[derive(Debug, AsChangeset)]
#[diesel(table_name = invites)]
pub struct UpdateInvite {
    pub updated_by: Option<UserId>,
    pub updated_at: Option<DateTime<Utc>>,
    pub room: Option<RoomId>,
    pub active: Option<bool>,
    pub expiration: Option<Option<DateTime<Utc>>>,
}

impl UpdateInvite {
    #[tracing::instrument(err, skip_all)]
    pub fn apply(
        self,
        conn: &mut DbConnection,
        room_id: RoomId,
        invite_code_id: InviteCodeId,
    ) -> Result<Invite> {
        let query = diesel::update(invites::table)
            .filter(
                invites::id
                    .eq(invite_code_id)
                    .and(invites::room.eq(room_id)),
            )
            .set(self)
            .returning(invites::all_columns);

        let invite = query.get_result(conn)?;

        Ok(invite)
    }
}
