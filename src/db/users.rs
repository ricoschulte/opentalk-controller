//! Contains the user specific database structs amd queries
use super::Result;
use crate::db::schema::users;
use crate::db::DbInterface;
use crate::diesel::ExpressionMethods;
use crate::diesel::QueryDsl;
use diesel::result::Error;
use diesel::{Identifiable, Queryable};
use diesel::{QueryResult, RunQueryDsl};
use uuid::Uuid;

/// Diesel user struct
///
/// Is used as a result in various queries. Represents a user column
#[derive(Debug, Clone, Queryable, Identifiable)]
pub struct User {
    pub id: i64,
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

impl DbInterface {
    pub fn create_user(&self, new_user: NewUser) -> Result<User> {
        let con = self.get_con()?;

        let user_result = diesel::insert_into(users::table)
            .values(new_user)
            .get_result(&con);

        match user_result {
            Ok(user) => Ok(user),
            Err(e) => {
                log::error!("Query error creating new user, {}", e);
                Err(e.into())
            }
        }
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

    pub fn modify_user(&self, user_uuid: Uuid, user: ModifyUser) -> Result<User> {
        let con = self.get_con()?;

        let target = users::table.filter(users::columns::oidc_uuid.eq(user_uuid));
        let user_result: QueryResult<User> = diesel::update(target).set(user).get_result(&con);

        match user_result {
            Ok(user) => Ok(user),
            Err(e) => {
                log::error!("Query error modifying user, {}", e);
                Err(e.into())
            }
        }
    }

    pub fn get_user_by_id(&self, user_id: i64) -> Result<Option<User>> {
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
