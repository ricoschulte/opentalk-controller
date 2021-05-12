#![allow(dead_code)] //TODO: remove this when some module implements the interface
use crate::db::models::{Room, RoomForm, User, UserForm};
use crate::db::schema::*;
use crate::diesel::ExpressionMethods;
use crate::diesel::QueryDsl;
use crate::settings;
use anyhow::{anyhow, Context, Result};
use diesel::result::Error;
use diesel::{Connection, PgConnection, QueryResult, RunQueryDsl};
use uuid::Uuid;

pub struct DbInterface {
    con: PgConnection,
    cfg: settings::Database,
}

impl DbInterface {
    /// Creates a new DbInterface instance with specified database settings and connects to the configured database.
    /// Uses the database settings which were provided on instantiation.
    pub fn connect(db_settings: settings::Database) -> Result<Self> {
        let con_uri = pg_connection_uri(&db_settings);

        //TODO: connection pooling
        let con = PgConnection::establish(&con_uri)
            .with_context(|| format!("Error connecting to database '{}'", con_uri))?;

        Ok(Self {
            con,
            cfg: db_settings,
        })
    }

    // ----------------- USER OPERATIONS -----------------

    /// Insert an user into the database.
    ///
    /// Returns the new user.
    pub fn user_insert(&self, user_form: UserForm) -> Result<User> {
        let user = diesel::insert_into(users::table)
            .values(&user_form)
            .get_result(&self.con)
            .with_context(|| format!("Unable to insert user into database '{:?}'", user_form))?;

        Ok(user)
    }

    /// Removes the user and all associated rooms from the database
    pub fn user_remove(&self, user: User) -> Result<()> {
        self.con.transaction(|| {
            // delete all owned rooms
            diesel::delete(rooms::table.filter(rooms::columns::owner.eq(user.id())))
                .execute(&self.con)?;

            // delete the user
            let rows_affected = diesel::delete(&user).execute(&self.con)?;

            if rows_affected < 1 {
                return Err(Error::RollbackTransaction);
            }
            Ok(())
        })?;

        Ok(())
    }

    /// Selects a user from the database.
    ///
    /// Returns Ok(None) if no user could be found.
    pub fn get_user_by_uuid(&self, uuid: &Uuid) -> Result<Option<User>> {
        let result: QueryResult<User> = users::table
            .filter(users::columns::oidc_uuid.eq(uuid))
            .get_result(&self.con);

        match result {
            Ok(user) => Ok(Some(user)),
            Err(Error::NotFound) => Ok(None),
            Err(e) => Err(anyhow!(e))
                .with_context(|| format!("Database error on select user by uuid '{}'", uuid)),
        }
    }

    /// Selects all rooms owned by the user
    pub fn user_get_owned_rooms(&self, user: User) -> Result<Vec<Room>> {
        let rooms: Vec<Room> = rooms::table
            .filter(rooms::columns::owner.eq(user.id()))
            .get_results(&self.con)
            .with_context(|| format!("Database error on select rooms by user id '{:?}'", user))?;

        Ok(rooms)
    }

    /// Updates all values of a user with the provided form, the user is selected by id.
    pub fn user_update_by_form(&self, user_form: UserForm, id: i64) -> Result<User> {
        let target = users::table.filter(users::columns::id.eq(id));
        let new_user: User = diesel::update(target)
            .set((users::columns::email.eq(&user_form.email()),))
            .get_result(&self.con)
            .with_context(|| format!("Unable to update user by form '{:?}'", user_form))?;

        Ok(new_user)
    }

    // ----------------- ROOM OPERATIONS -----------------

    /// Insert a room into the database.
    ///
    /// Returns the new room.
    pub fn room_insert(&self, room_form: RoomForm) -> Result<Room> {
        let room = diesel::insert_into(rooms::table)
            .values(&room_form)
            .get_result(&self.con)
            .with_context(|| format!("Unable to insert room into database '{:?}'", room_form))?;

        Ok(room)
    }

    /// Removes the room from the database
    pub fn room_delete(&self, room: Room) -> Result<usize> {
        let rows_affected = diesel::delete(&room).execute(&self.con)?;

        Ok(rows_affected)
    }

    /// Selects a room from the database.
    ///
    /// Returns Ok(None) if no room could be found.
    pub fn get_room_by_id(&self, id: i64) -> Result<Option<Room>> {
        let result: QueryResult<Room> = rooms::table
            .filter(rooms::columns::id.eq(id))
            .get_result(&self.con);

        match result {
            Ok(room) => Ok(Some(room)),
            Err(Error::NotFound) => Ok(None),
            Err(e) => Err(anyhow!(e))
                .with_context(|| format!("Database error on select room by id '{}'", id)),
        }
    }

    /// Selects the owner (User) of this room.
    pub fn room_get_owner(&self, room: &Room) -> Result<User> {
        let user: User = users::table
            .filter(users::columns::id.eq(room.owner()))
            .get_result(&self.con)?;

        Ok(user)
    }

    /// Updates all values of a room with the provided form, the room is selected by id.
    pub fn room_update_by_form(&self, room_form: RoomForm, id: i64) -> Result<Room> {
        let target = rooms::table.filter(rooms::columns::id.eq(id));
        let new_room: Room = diesel::update(target)
            .set((
                rooms::columns::owner.eq(room_form.owner()),
                rooms::columns::password.eq(&room_form.password()),
                rooms::columns::wait_for_moderator.eq(room_form.wait_for_moderator()),
                rooms::columns::listen_only.eq(room_form.listen_only()),
            ))
            .get_result(&self.con)
            .with_context(|| format!("Unable to update room by form '{:?}'", room_form))?;

        Ok(new_room)
    }
}

fn pg_connection_uri(cfg: &settings::Database) -> String {
    format!(
        "postgres://{}:{}@{}:{}/{}",
        cfg.user, cfg.password, cfg.server, cfg.port, cfg.name
    )
}
