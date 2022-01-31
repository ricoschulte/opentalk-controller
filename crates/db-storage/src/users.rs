//! Contains the user specific database structs amd queries
use super::groups::{DbGroupsEx, Group, UserGroup};
use super::schema::{groups, user_groups, users};
use crate::groups::NewUserGroup;
use crate::{levenshtein, lower, soundex};
use controller_shared::{impl_from_redis_value_de, impl_to_redis_args_se};
use database::{DatabaseError, DbInterface, Paginate, Result};
use diesel::dsl::any;
use diesel::r2d2::{ConnectionManager, PooledConnection};
use diesel::result::Error;
use diesel::{
    BelongingToDsl, BoolExpressionMethods, Connection, ExpressionMethods, GroupedBy, Identifiable,
    Insertable, PgConnection, QueryDsl, QueryResult, Queryable, RunQueryDsl, TextExpressionMethods,
};
use uuid::Uuid;

diesel_newtype! {
    #[derive(Copy)] SerialUserId(i64) => diesel::sql_types::BigInt, "diesel::sql_types::BigInt", "/users/",
    #[derive(Copy)] UserId(uuid::Uuid) => diesel::sql_types::Uuid, "diesel::sql_types::Uuid"
}

impl_to_redis_args_se!(SerialUserId);
impl_from_redis_value_de!(SerialUserId);

/// Diesel user struct
///
/// Is used as a result in various queries. Represents a user column
#[derive(Clone, Queryable, Identifiable)]
pub struct User {
    pub id: SerialUserId,
    pub oidc_uuid: Uuid,
    pub email: String,
    pub title: String,
    pub firstname: String,
    pub lastname: String,
    pub id_token_exp: i64,
    pub theme: String,
    pub language: String,
}

/// Diesel insertable user struct
///
/// Represents fields that have to be provided on user insertion.
#[derive(Insertable)]
#[table_name = "users"]
pub struct NewUser {
    pub oidc_uuid: Uuid,
    pub email: String,
    pub title: String,
    pub firstname: String,
    pub lastname: String,
    pub id_token_exp: i64,
    pub theme: String,
    pub language: String,
}

pub struct NewUserWithGroups {
    pub new_user: NewUser,
    pub groups: Vec<String>,
}

/// Diesel user struct for updates
///
/// Is used in update queries. None fields will be ignored on update queries
#[derive(AsChangeset)]
#[table_name = "users"]
pub struct UpdateUser {
    pub title: Option<String>,
    pub theme: Option<String>,
    pub language: Option<String>,
    pub id_token_exp: Option<i64>,
}

/// Ok type of [`DbUsersEx::modify_user`]
pub struct ModifiedUser {
    /// The user after the modification
    pub user: User,

    /// True the user's groups changed.
    /// Relevant for permission related state
    pub groups_changed: bool,

    pub groups_added: Vec<String>,
    pub groups_removed: Vec<String>,
}

pub trait DbUsersEx: DbInterface + DbGroupsEx {
    #[tracing::instrument(err, skip_all)]
    fn create_user(&self, new_user: NewUserWithGroups) -> Result<User> {
        let conn = self.get_conn()?;

        conn.transaction::<User, DatabaseError, _>(|| {
            let user: User = diesel::insert_into(users::table)
                .values(new_user.new_user)
                .get_result(&conn)?;

            insert_user_into_user_groups(&conn, &user, new_user.groups)?;

            Ok(user)
        })
    }

    #[tracing::instrument(err, skip_all)]
    fn get_users_with_groups(&self) -> Result<Vec<(User, Vec<Group>)>> {
        let conn = self.get_conn()?;

        let users_with_groups = || -> Result<_> {
            let user_query = users::table.order_by(users::columns::id.desc());
            let users = user_query.load::<User>(&conn)?;

            let groups: Vec<Vec<(UserGroup, Group)>> = UserGroup::belonging_to(&users)
                .inner_join(groups::table)
                .load::<(UserGroup, Group)>(&conn)?
                .grouped_by(&users);

            Ok(users
                .into_iter()
                .zip(groups)
                .map(|(user, groups)| (user, groups.into_iter().map(|(_, group)| group).collect()))
                .collect::<Vec<_>>())
        }()?;

        Ok(users_with_groups)
    }

    #[tracing::instrument(err, skip_all, fields(%limit, %page))]
    fn get_users_paginated(&self, limit: i64, page: i64) -> Result<(Vec<User>, i64)> {
        let conn = self.get_conn()?;

        let query = users::table
            .order_by(users::columns::id.desc())
            .paginate_by(limit, page);

        let users_with_total = query.load_and_count::<User, _>(&conn)?;

        Ok(users_with_total)
    }

    #[tracing::instrument(err, skip_all, fields(%limit, %page))]
    fn get_users_by_ids_paginated(
        &self,
        ids: &[SerialUserId],
        limit: i64,
        page: i64,
    ) -> Result<(Vec<User>, i64)> {
        let conn = self.get_conn()?;

        let query = users::table
            .filter(users::columns::id.eq(any(ids)))
            .order_by(users::columns::id.desc())
            .paginate_by(limit, page);

        let users_with_total = query.load_and_count::<User, _>(&conn)?;

        Ok(users_with_total)
    }

    #[tracing::instrument(err, skip_all)]
    fn get_users_by_ids(&self, ids: &[SerialUserId]) -> Result<Vec<User>> {
        let conn = self.get_conn()?;

        let result: QueryResult<Vec<User>> = users::table
            .filter(users::columns::id.eq_any(ids))
            .get_results(&conn);

        match result {
            Ok(users) => Ok(users),
            Err(Error::NotFound) => Ok(vec![]),
            Err(e) => Err(e.into()),
        }
    }

    #[tracing::instrument(err, skip_all)]
    fn get_user_by_uuid(&self, uuid: &Uuid) -> Result<Option<User>> {
        let conn = self.get_conn()?;

        let result: QueryResult<User> = users::table
            .filter(users::columns::oidc_uuid.eq(uuid))
            .get_result(&conn);

        match result {
            Ok(user) => Ok(Some(user)),
            Err(Error::NotFound) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    #[tracing::instrument(err, skip_all)]
    fn update_user(
        &self,
        user_uuid: Uuid,
        modify: UpdateUser,
        groups: Option<Vec<String>>,
    ) -> Result<ModifiedUser> {
        let conn = self.get_conn()?;

        conn.transaction::<ModifiedUser, DatabaseError, _>(|| {
            let target = users::table.filter(users::columns::oidc_uuid.eq(user_uuid));
            let user: User = diesel::update(target).set(modify).get_result(&conn)?;

            // modify groups if parameter exists
            if let Some(groups) = groups {
                let curr_groups = self.get_groups_for_user(user.id)?;

                // check current groups and if reinsert of groups into user_groups is needed
                let (added, removed) = (
                    groups
                        .iter()
                        .filter(|&old| !curr_groups.contains(old))
                        .cloned()
                        .collect::<Vec<_>>(),
                    curr_groups
                        .iter()
                        .filter(|&curr| !groups.contains(&curr.id))
                        .map(|g| g.id.clone())
                        .collect::<Vec<_>>(),
                );
                let groups_changed = !added.is_empty() || !removed.is_empty();

                if groups_changed {
                    // Remove user from user_groups table
                    let target =
                        user_groups::table.filter(user_groups::columns::user_id.eq(user.id));
                    diesel::delete(target).execute(&conn).map_err(|e| {
                        log::error!("Failed to remove user's groups from user_groups, {}", e);
                        DatabaseError::from(e)
                    })?;

                    insert_user_into_user_groups(&conn, &user, groups)?;
                }

                Ok(ModifiedUser {
                    user,
                    groups_changed,
                    groups_added: added,
                    groups_removed: removed,
                })
            } else {
                Ok(ModifiedUser {
                    user,
                    groups_changed: false,
                    groups_added: vec![],
                    groups_removed: vec![],
                })
            }
        })
    }

    #[tracing::instrument(err, skip_all)]
    fn get_opt_user_by_id(&self, user_id: SerialUserId) -> Result<Option<User>> {
        let conn = self.get_conn()?;

        let result: QueryResult<User> = users::table
            .filter(users::columns::id.eq(user_id))
            .get_result(&conn);

        match result {
            Ok(user) => Ok(Some(user)),
            Err(Error::NotFound) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    #[tracing::instrument(err, skip_all)]
    fn get_user_by_id(&self, user_id: SerialUserId) -> Result<User> {
        match self.get_opt_user_by_id(user_id) {
            Ok(Some(user)) => Ok(user),
            Ok(None) => Err(Error::NotFound.into()),
            Err(e) => Err(e),
        }
    }

    #[tracing::instrument(err, skip_all)]
    fn find_users_by_name(&self, search_str: &str) -> Result<Vec<User>> {
        // IMPORTANT: lowercase it to match the index of the db and
        // remove all existing % in name and to avoid manipulation of the LIKE query.
        let search_str = search_str.replace('%', "").trim().to_lowercase();

        if search_str.is_empty() {
            return Ok(vec![]);
        }

        let like_query = format!("%{}%", search_str);

        let conn = self.get_conn()?;

        let lower_first_lastname = lower(
            users::columns::firstname
                .concat(" ")
                .concat(users::columns::lastname),
        );

        let matches = users::table
            .filter(
                lower_first_lastname
                    .like(&like_query)
                    .or(lower(users::columns::email).like(&like_query))
                    .or(soundex(lower_first_lastname)
                        .eq(soundex(&search_str))
                        .and(levenshtein(lower_first_lastname, &search_str).lt(5))),
            )
            .order_by(levenshtein(lower_first_lastname, &search_str))
            .then_order_by(users::columns::id)
            .limit(5)
            .get_results(&conn)?;

        Ok(matches)
    }
}
impl<T: DbInterface> DbUsersEx for T {}

#[tracing::instrument(err, skip_all)]
fn insert_user_into_user_groups(
    con: &PooledConnection<ConnectionManager<PgConnection>>,
    user: &User,
    groups: Vec<String>,
) -> Result<()> {
    let possibly_new_groups = groups
        .iter()
        .cloned()
        .map(|id| Group { id })
        .collect::<Vec<Group>>();

    diesel::insert_into(groups::table)
        .values(possibly_new_groups)
        .on_conflict(groups::id)
        .do_nothing()
        .execute(con)
        .map_err(|e| {
            log::error!("Failed to insert possibly new groups, {}", e);
            DatabaseError::from(e)
        })?;

    let user_groups_ = groups
        .into_iter()
        .map(|group_id| NewUserGroup {
            user_id: user.id,
            group_id,
        })
        .collect::<Vec<NewUserGroup>>();

    diesel::insert_into(user_groups::table)
        .values(&user_groups_)
        .execute(con)
        .map_err(|e| {
            log::error!("Failed to insert user_groups, {}", e);
            DatabaseError::from(e)
        })?;

    Ok(())
}
