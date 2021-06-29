use super::schema::{groups, user_groups};
use super::{DbInterface, Result};
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};

#[derive(Debug, Queryable, Insertable)]
#[table_name = "groups"]
pub struct Group {
    pub id: String,
}

#[derive(Debug, Queryable, Insertable)]
#[table_name = "user_groups"]
pub struct UserGroup {
    pub user_id: i64,
    pub group_id: String,
}

impl DbInterface {
    pub fn get_groups_for_user(&self, user_id: i64) -> Result<Vec<Group>> {
        let con = self.get_con()?;

        user_groups::table
            .inner_join(groups::table)
            .filter(user_groups::user_id.eq(user_id))
            .select(groups::all_columns)
            .get_results(&con)
            .map_err(|e| {
                log::error!("Failed to get groups for user, {}", e);
                e.into()
            })
    }
}
