//! Contains the user specific database structs amd queries
use super::groups::{DbGroupsEx, Group, UserGroup};
use super::schema::{groups, user_groups, users};
use controller_shared::{impl_from_redis_value_de, impl_to_redis_args_se};
use database::{DatabaseError, DbInterface, Result};
use diesel::r2d2::{self, ConnectionManager};
use diesel::result::Error;
use diesel::{
    Connection, ExpressionMethods, Identifiable, Insertable, PgConnection, QueryDsl, QueryResult,
    Queryable, RunQueryDsl,
};
use uuid::Uuid;

diesel_newtype!(#[derive(Copy)] UserId(i64) => diesel::sql_types::BigInt, "diesel::sql_types::BigInt");

impl_to_redis_args_se!(UserId);
impl_from_redis_value_de!(UserId);

/// Diesel user struct
///
/// Is used as a result in various queries. Represents a user column
#[derive(Clone, Queryable, Identifiable)]
pub struct User {
    pub id: UserId,
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
pub struct ModifyUser {
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
    #[tracing::instrument(skip(self, new_user))]
    fn create_user(&self, new_user: NewUserWithGroups) -> Result<User> {
        let con = self.get_conn()?;

        con.transaction::<User, DatabaseError, _>(|| {
            let user: User = diesel::insert_into(users::table)
                .values(new_user.new_user)
                .get_result(&con)
                .map_err(|e| {
                    log::error!("Failed to create user, {}", e);
                    DatabaseError::from(e)
                })?;

            insert_user_into_user_groups(&con, &user, new_user.groups)?;

            Ok(user)
        })
    }

    #[tracing::instrument(skip(self))]
    fn get_users(&self) -> Result<Vec<User>> {
        let con = self.get_conn()?;

        let users_result: QueryResult<Vec<User>> = users::table.get_results(&con);

        match users_result {
            Ok(users) => Ok(users),
            Err(e) => {
                log::error!("Query error getting all users, {}", e);
                Err(e.into())
            }
        }
    }

    #[tracing::instrument(skip(self, uuid))]
    fn get_user_by_uuid(&self, uuid: &Uuid) -> Result<Option<User>> {
        let con = self.get_conn()?;

        let result: QueryResult<User> = users::table
            .filter(users::columns::oidc_uuid.eq(uuid))
            .get_result(&con);

        match result {
            Ok(user) => Ok(Some(user)),
            Err(Error::NotFound) => Ok(None),
            Err(e) => {
                log::error!("Query error getting user by uuid, {}", e);
                Err(e.into())
            }
        }
    }

    #[tracing::instrument(skip(self, user_uuid, modify, groups))]
    fn modify_user(
        &self,
        user_uuid: Uuid,
        modify: ModifyUser,
        groups: Option<Vec<String>>,
    ) -> Result<ModifiedUser> {
        let con = self.get_conn()?;

        con.transaction::<ModifiedUser, DatabaseError, _>(|| {
            let target = users::table.filter(users::columns::oidc_uuid.eq(user_uuid));
            let user: User = diesel::update(target).set(modify).get_result(&con)?;

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
                    diesel::delete(target).execute(&con).map_err(|e| {
                        log::error!("Failed to remove user's groups from user_groups, {}", e);
                        DatabaseError::from(e)
                    })?;

                    insert_user_into_user_groups(&con, &user, groups)?;
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

    #[tracing::instrument(skip(self, user_id))]
    fn get_user_by_id(&self, user_id: UserId) -> Result<Option<User>> {
        let con = self.get_conn()?;

        let result: QueryResult<User> = users::table
            .filter(users::columns::id.eq(user_id))
            .get_result(&con);

        match result {
            Ok(user) => Ok(Some(user)),
            Err(Error::NotFound) => Ok(None),
            Err(e) => {
                log::error!("Query error getting user by id, {}", e);

                Err(e.into())
            }
        }
    }
}
impl<T: DbInterface> DbUsersEx for T {}

fn insert_user_into_user_groups(
    con: &r2d2::PooledConnection<ConnectionManager<PgConnection>>,
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
        .map(|group_id| UserGroup {
            user_id: user.id,
            group_id,
        })
        .collect::<Vec<UserGroup>>();

    diesel::insert_into(user_groups::table)
        .values(&user_groups_)
        .execute(con)
        .map_err(|e| {
            log::error!("Failed to insert user_groups, {}", e);
            DatabaseError::from(e)
        })?;

    Ok(())
}
