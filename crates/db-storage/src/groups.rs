use super::schema::{groups, user_groups};
use super::users::{User, UserId};
use controller_shared::{impl_from_redis_value_de, impl_to_redis_args_se};
use database::{DbInterface, Result};
use diesel::{
    BoolExpressionMethods, Connection, ExpressionMethods, Identifiable, Insertable,
    OptionalExtension, QueryDsl, Queryable, RunQueryDsl,
};
use kustos::subject::PolicyGroup;
use std::borrow::Borrow;
use std::collections::HashSet;

diesel_newtype! {
    #[derive(Copy)] GroupId(uuid::Uuid) => diesel::sql_types::Uuid, "diesel::sql_types::Uuid",
    #[derive(Copy)] SerialGroupId(i64) => diesel::sql_types::BigInt, "diesel::sql_types::BigInt"
}

impl_to_redis_args_se!(GroupId);
impl_from_redis_value_de!(GroupId);

impl From<GroupId> for PolicyGroup {
    fn from(group_id: GroupId) -> Self {
        Self::from(group_id.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Queryable, Insertable, Identifiable)]
#[table_name = "groups"]
pub struct Group {
    pub id: GroupId,
    pub id_serial: SerialGroupId,
    pub oidc_issuer: Option<String>,
    pub name: String,
}

impl Borrow<String> for Group {
    fn borrow(&self) -> &String {
        &self.name
    }
}

impl Borrow<str> for Group {
    fn borrow(&self) -> &str {
        &self.name
    }
}

#[derive(Debug, Insertable)]
#[table_name = "groups"]
pub struct NewGroup {
    // TODO make this non-null
    pub oidc_issuer: Option<String>,
    pub name: String,
}

#[derive(Debug, Insertable)]
#[table_name = "user_groups"]
pub struct NewUserGroup {
    pub user_id: UserId,
    pub group_id: GroupId,
}

#[derive(Debug, Queryable, Identifiable, Associations)]
#[table_name = "user_groups"]
#[belongs_to(User, foreign_key = "user_id")]
#[belongs_to(Group, foreign_key = "group_id")]
#[primary_key(user_id, group_id)]
pub struct UserGroup {
    pub user_id: UserId,
    pub group_id: GroupId,
}

pub trait DbGroupsEx: DbInterface {
    #[tracing::instrument(err, skip_all)]
    fn get_or_create_group(&self, new_group: NewGroup) -> Result<Group> {
        let conn = self.get_conn()?;

        conn.transaction(|| {
            let group: Option<Group> = groups::table
                .select(groups::all_columns)
                .filter(
                    groups::oidc_issuer
                        .eq(&new_group.oidc_issuer)
                        .and(groups::name.eq(&new_group.name)),
                )
                .first(&conn)
                .optional()?;

            let group = if let Some(group) = group {
                group
            } else {
                diesel::insert_into(groups::table)
                    .values(new_group)
                    .get_result(&conn)?
            };

            Ok(group)
        })
    }

    #[tracing::instrument(err, skip_all)]
    fn get_groups_for_user(&self, user_id: UserId) -> Result<HashSet<Group>> {
        let conn = self.get_conn()?;

        let groups: Vec<Group> = user_groups::table
            .inner_join(groups::table)
            .filter(user_groups::user_id.eq(user_id))
            .select(groups::all_columns)
            .order_by(groups::id_serial)
            .get_results(&conn)?;

        Ok(groups.into_iter().collect())
    }
}

impl<T: DbInterface> DbGroupsEx for T {}
