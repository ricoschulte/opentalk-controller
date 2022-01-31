use super::schema::{groups, user_groups};
use super::users::{SerialUserId, User};
use database::{DbInterface, Result};
use diesel::{ExpressionMethods, Identifiable, Insertable, QueryDsl, Queryable, RunQueryDsl};
use std::borrow::Borrow;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Queryable, Insertable, Identifiable)]
#[table_name = "groups"]
pub struct Group {
    pub id: String,
}

impl Borrow<str> for Group {
    fn borrow(&self) -> &str {
        &self.id
    }
}

impl Borrow<String> for Group {
    fn borrow(&self) -> &String {
        &self.id
    }
}

#[derive(Debug, Insertable)]
#[table_name = "user_groups"]
pub struct NewUserGroup {
    pub user_id: SerialUserId,
    pub group_id: String,
}

#[derive(Debug, Queryable, Identifiable, Associations)]
#[table_name = "user_groups"]
#[belongs_to(User, foreign_key = "user_id")]
#[belongs_to(Group, foreign_key = "group_id")]
#[primary_key(user_id, group_id)]
pub struct UserGroup {
    pub user_id: SerialUserId,
    pub group_id: String,
}

pub trait DbGroupsEx: DbInterface {
    #[tracing::instrument(err, skip_all)]
    fn get_groups_for_user(&self, user_id: SerialUserId) -> Result<HashSet<Group>> {
        let con = self.get_conn()?;

        let groups: Vec<Group> = user_groups::table
            .inner_join(groups::table)
            .filter(user_groups::user_id.eq(user_id))
            .select(groups::all_columns)
            .order_by(groups::id)
            .get_results(&con)?;

        Ok(groups.into_iter().collect())
    }
}

impl<T: DbInterface> DbGroupsEx for T {}
