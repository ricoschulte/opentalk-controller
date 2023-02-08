// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2
use super::schema::{groups, user_groups};
use super::users::{User, UserId};
use core::convert::Infallible;
use core::str::FromStr;
use database::{DbConnection, Result};
use diesel::prelude::*;
use kustos::subject::PolicyGroup;

diesel_newtype! {
    #[derive(Copy, redis_args::ToRedisArgs, redis_args::FromRedisValue)]
    #[to_redis_args(serde)]
    #[from_redis_value(serde)]
    GroupId(uuid::Uuid) => diesel::sql_types::Uuid,

    #[derive(Copy)]
    SerialGroupId(i64) => diesel::sql_types::BigInt,

    #[derive(redis_args::ToRedisArgs, redis_args::FromRedisValue)]
    #[to_redis_args(fmt = "{0}")]
    #[from_redis_value(FromStr)]
    GroupName(String) => diesel::sql_types::Text
}

impl FromStr for GroupName {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from(s.into()))
    }
}

impl From<GroupId> for PolicyGroup {
    fn from(group_id: GroupId) -> Self {
        Self::from(group_id.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Queryable, Insertable, Identifiable)]
#[diesel(table_name = groups)]
pub struct Group {
    pub id: GroupId,
    pub id_serial: SerialGroupId,
    pub name: GroupName,
}

impl Group {
    #[tracing::instrument(err, skip_all)]
    pub fn get_all_for_user(conn: &mut DbConnection, user_id: UserId) -> Result<Vec<Group>> {
        let query = user_groups::table
            .inner_join(groups::table)
            .filter(user_groups::user_id.eq(user_id))
            .select(groups::all_columns)
            .order_by(groups::id_serial);

        let groups: Vec<Group> = query.load(conn)?;

        Ok(groups)
    }
}
#[derive(Debug, Insertable)]
#[diesel(table_name = groups)]
pub struct NewGroup<'a> {
    pub name: &'a GroupName,
}

impl NewGroup<'_> {
    /// Insert the new group. If the group already exists for the OIDC issuer the group will be returned instead
    #[tracing::instrument(err, skip_all)]
    pub fn insert_or_get(self, conn: &mut DbConnection) -> Result<Group> {
        conn.transaction(|conn| {
            let query = groups::table
                .select(groups::all_columns)
                .filter(groups::name.eq(&self.name));

            let group: Option<Group> = query.first(conn).optional()?;

            let group = if let Some(group) = group {
                group
            } else {
                diesel::insert_into(groups::table)
                    .values(self)
                    .get_result(conn)?
            };

            Ok(group)
        })
    }
}

#[derive(Debug, Insertable)]
#[diesel(table_name = user_groups)]
pub struct NewUserGroupRelation {
    pub user_id: UserId,
    pub group_id: GroupId,
}

#[derive(Debug, Queryable, Identifiable, Associations)]
#[diesel(table_name = user_groups)]
#[diesel(belongs_to(User, foreign_key = user_id))]
#[diesel(belongs_to(Group, foreign_key = group_id))]
#[diesel(primary_key(user_id, group_id))]
pub struct UserGroupRelation {
    pub user_id: UserId,
    pub group_id: GroupId,
}

/// Get or create groups in the database by their name
/// If the group is currently not stored, create a new group and returns the ID along the already present ones.
/// Does not preserve the order of groups passed to the function
pub fn get_or_create_groups_by_name(
    conn: &mut DbConnection,
    groups: &[GroupName],
) -> Result<Vec<Group>> {
    let query = groups::table
        .select(groups::all_columns)
        .filter(groups::name.eq_any(groups));

    let mut present_groups: Vec<Group> = query.load(conn)?;

    // Create a `NewGroup` for every group that the previous query didn't return
    let new_groups: Vec<NewGroup> = groups
        .iter()
        .filter(|wanted_group_name| {
            !present_groups
                .iter()
                .any(|present_group| present_group.name == **wanted_group_name)
        })
        .map(|name| NewGroup { name })
        .collect();

    if !new_groups.is_empty() {
        // Insert new groups and return them
        let new_groups: Vec<Group> = diesel::insert_into(groups::table)
            .values(&new_groups)
            .returning(groups::all_columns)
            .load(conn)?;

        present_groups.extend(new_groups);
    }

    Ok(present_groups)
}

#[tracing::instrument(err, skip_all)]
pub fn insert_user_into_groups(
    conn: &mut DbConnection,
    user: &User,
    groups: &[Group],
) -> Result<()> {
    let new_user_groups = groups
        .iter()
        .map(|group| NewUserGroupRelation {
            user_id: user.id,
            group_id: group.id,
        })
        .collect::<Vec<_>>();

    diesel::insert_into(user_groups::table)
        .values(new_user_groups)
        .on_conflict_do_nothing()
        .execute(conn)?;

    Ok(())
}

#[tracing::instrument(err, skip_all)]
pub fn remove_user_from_groups(
    conn: &mut DbConnection,
    user: &User,
    groups: &[Group],
) -> Result<()> {
    let group_ids: Vec<GroupId> = groups.iter().map(|group| group.id).collect();

    diesel::delete(user_groups::table)
        .filter(
            user_groups::user_id
                .eq(user.id)
                .and(user_groups::group_id.eq_any(group_ids)),
        )
        .execute(conn)?;

    Ok(())
}
