use super::schema::{groups, user_groups};
use super::users::{User, UserId};
use controller_shared::{impl_from_redis_value_de, impl_to_redis_args_se};
use database::{DbConnection, Result};
use diesel::{
    BoolExpressionMethods, Connection, ExpressionMethods, Identifiable, Insertable,
    OptionalExtension, QueryDsl, Queryable, RunQueryDsl,
};
use kustos::subject::PolicyGroup;

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
    pub oidc_issuer: String,
    pub name: String,
}

impl Group {
    #[tracing::instrument(err, skip_all)]
    pub fn get_all_for_user(conn: &DbConnection, user_id: UserId) -> Result<Vec<Group>> {
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
#[table_name = "groups"]
pub struct NewGroup {
    pub oidc_issuer: String,
    pub name: String,
}

impl NewGroup {
    /// Insert the new group. If the group already exists for the OIDC issuer the group will be returned instead
    #[tracing::instrument(err, skip_all)]
    pub fn insert_or_get(self, conn: &DbConnection) -> Result<Group> {
        conn.transaction(|| {
            let query = groups::table.select(groups::all_columns).filter(
                groups::oidc_issuer
                    .eq(&self.oidc_issuer)
                    .and(groups::name.eq(&self.name)),
            );

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
#[table_name = "user_groups"]
pub struct NewUserGroupRelation {
    pub user_id: UserId,
    pub group_id: GroupId,
}

#[derive(Debug, Queryable, Identifiable, Associations)]
#[table_name = "user_groups"]
#[belongs_to(User, foreign_key = "user_id")]
#[belongs_to(Group, foreign_key = "group_id")]
#[primary_key(user_id, group_id)]
pub struct UserGroupRelation {
    pub user_id: UserId,
    pub group_id: GroupId,
}
