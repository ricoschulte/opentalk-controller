use super::schema::{groups, user_groups};
use super::users::UserId;
use super::{DatabaseError, DbInterface, Result};
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use std::borrow::Borrow;
use std::collections::HashSet;

#[derive(Debug, PartialEq, Eq, Hash, Queryable, Insertable)]
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

#[derive(Debug, Queryable, Insertable)]
#[table_name = "user_groups"]
pub struct UserGroup {
    pub user_id: UserId,
    pub group_id: String,
}

impl DbInterface {
    pub fn get_groups_for_user(&self, user_id: UserId) -> Result<HashSet<Group>> {
        let con = self.get_con()?;

        let groups: Vec<Group> = user_groups::table
            .inner_join(groups::table)
            .filter(user_groups::user_id.eq(user_id))
            .select(groups::all_columns)
            .get_results(&con)
            .map_err(|e| {
                log::error!("Failed to get groups for user, {}", e);
                DatabaseError::from(e)
            })?;

        Ok(groups.into_iter().collect())
    }
}
