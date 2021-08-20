//! Contains the user specific database structs amd queries
use super::groups::{Group, UserGroup};
use super::schema::{groups, user_groups, users};
use super::{DatabaseError, DbConnection, DbInterface, Result};
use diesel::result::Error;
use diesel::{
    Connection, ExpressionMethods, Identifiable, Insertable, QueryDsl, QueryResult, Queryable,
    RunQueryDsl,
};
use uuid::Uuid;

diesel_newtype!(UserId(i64) => diesel::sql_types::BigInt, "diesel::sql_types::BigInt");

/// Diesel user struct
///
/// Is used as a result in various queries. Represents a user column
#[derive(Debug, Clone, Queryable, Identifiable)]
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
#[derive(Debug, Insertable)]
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

#[derive(Debug)]
pub struct NewUserWithGroups {
    pub new_user: NewUser,
    pub groups: Vec<String>,
}

/// Diesel user struct for updates
///
/// Is used in update queries. None fields will be ignored on update queries
#[derive(Debug, AsChangeset)]
#[table_name = "users"]
pub struct ModifyUser {
    pub title: Option<String>,
    pub theme: Option<String>,
    pub language: Option<String>,
    pub id_token_exp: Option<i64>,
}

/// Ok type of [`DbInterface::modify_user`]
pub struct ModifiedUser {
    /// The user after the modification
    pub user: User,

    /// True the user's groups changed.
    /// Relevant for permission related state
    pub groups_changed: bool,
}

impl DbInterface {
    pub fn create_user(&self, new_user: NewUserWithGroups) -> Result<()> {
        let con = self.get_con()?;

        con.transaction::<(), super::DatabaseError, _>(|| {
            let user: User = diesel::insert_into(users::table)
                .values(new_user.new_user)
                .get_result(&con)
                .map_err(|e| {
                    log::error!("Failed to create user, {}", e);
                    DatabaseError::from(e)
                })?;

            insert_user_into_user_groups(&con, &user, new_user.groups)?;

            Ok(())
        })
    }

    pub fn get_users(&self) -> Result<Vec<User>> {
        let con = self.get_con()?;

        let users_result: QueryResult<Vec<User>> = users::table.get_results(&con);

        match users_result {
            Ok(users) => Ok(users),
            Err(e) => {
                log::error!("Query error getting all users, {}", e);
                Err(e.into())
            }
        }
    }

    pub fn get_user_by_uuid(&self, uuid: &Uuid) -> Result<Option<User>> {
        let con = self.get_con()?;

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

    pub fn modify_user(
        &self,
        user_uuid: Uuid,
        modify: ModifyUser,
        groups: Option<Vec<String>>,
    ) -> Result<ModifiedUser> {
        let con = self.get_con()?;

        con.transaction::<ModifiedUser, super::DatabaseError, _>(|| {
            let target = users::table.filter(users::columns::oidc_uuid.eq(user_uuid));
            let user: User = diesel::update(target).set(modify).get_result(&con)?;

            // modify groups if parameter exists
            if let Some(groups) = groups {
                let curr_groups = self.get_groups_for_user(user.id)?;

                // check current groups and if reinsert of groups into user_groups is needed
                let groups_unchanged = if groups.len() == curr_groups.len() {
                    groups.iter().all(|old| curr_groups.contains(old))
                } else {
                    false
                };

                if !groups_unchanged {
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
                    groups_changed: !groups_unchanged,
                })
            } else {
                Ok(ModifiedUser {
                    user,
                    groups_changed: false,
                })
            }
        })
    }

    pub fn get_user_by_id(&self, user_id: UserId) -> Result<Option<User>> {
        let con = self.get_con()?;

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

fn insert_user_into_user_groups(
    con: &DbConnection,
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
