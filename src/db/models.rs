//! This module contains all database tables as structs.
//!
//! The Object-relational mapping is done with Diesel

use diesel::{Identifiable, Queryable};

use super::schema::*;
use uuid::Uuid;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error, Clone)]
pub enum Error {
    #[error("Validation Error: `{0}`")]
    ValidationError(String),
}

#[derive(Debug, Insertable)]
#[table_name = "users"]
pub struct UserForm {
    oidc_uuid: Uuid,
    email: String,
}

impl UserForm {
    pub fn new(oidc_uuid: Uuid, email: String) -> Result<Self> {
        Ok(UserForm {
            oidc_uuid,
            email: Self::validate_email(email)?,
        })
    }

    pub fn oidc_uuid(&self) -> Uuid {
        self.oidc_uuid
    }

    pub fn email(&self) -> &str {
        &self.email
    }

    pub fn set_oidc_uuid(&mut self, oidc_uuid: Uuid) {
        self.oidc_uuid = oidc_uuid;
    }

    pub fn set_email(&mut self, email: String) -> Result<()> {
        self.email = Self::validate_email(email)?;
        Ok(())
    }

    fn validate_email(email: String) -> Result<String> {
        //TODO: validate email
        Ok(email)
    }
}

#[derive(Debug, Queryable, Identifiable, AsChangeset)]
pub struct User {
    id: i64,
    oidc_uuid: Uuid,
    email: String,
}

impl User {
    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn oidc_uuid(&self) -> Uuid {
        self.oidc_uuid
    }

    pub fn email(&self) -> &str {
        &self.email
    }
}

#[derive(Debug, Insertable)]
#[table_name = "rooms"]
pub struct RoomForm {
    owner: i64,
    password: String,
    wait_for_moderator: bool,
    listen_only: bool,
}

impl RoomForm {
    pub fn new(
        owner: i64,
        password: String,
        wait_for_moderator: bool,
        listen_only: bool,
    ) -> Result<Self> {
        Ok(RoomForm {
            owner,
            password: Self::validate_password(password)?,
            wait_for_moderator,
            listen_only,
        })
    }

    pub fn owner(&self) -> i64 {
        self.owner
    }

    pub fn password(&self) -> &str {
        &self.password
    }

    pub fn wait_for_moderator(&self) -> bool {
        self.wait_for_moderator
    }

    pub fn listen_only(&self) -> bool {
        self.listen_only
    }

    pub fn set_owner(&mut self, owner: i64) {
        self.owner = owner;
    }

    pub fn set_password(&mut self, password: String) -> Result<()> {
        self.password = Self::validate_password(password)?;
        Ok(())
    }

    pub fn set_wait_for_moderator(&mut self, wait_for_moderator: bool) {
        self.wait_for_moderator = wait_for_moderator;
    }

    pub fn set_listen_only(&mut self, listen_only: bool) {
        self.listen_only = listen_only;
    }

    fn validate_password(password: String) -> Result<String> {
        if password.len() > 128 {
            return Err(Error::ValidationError(
                "Password is too long, a maximum of 128 Characters is allowed".to_string(),
            ));
        }
        Ok(password)
    }
}

#[derive(Debug, Queryable, Identifiable, AsChangeset)]
pub struct Room {
    id: i64,
    owner: i64,
    password: String,
    wait_for_moderator: bool,
    listen_only: bool,
}

impl Room {
    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn owner(&self) -> i64 {
        self.owner
    }

    pub fn password(&self) -> &str {
        &self.password
    }

    pub fn wait_for_moderator(&self) -> bool {
        self.wait_for_moderator
    }

    pub fn listen_only(&self) -> bool {
        self.listen_only
    }
}
