//! Contains the room specific database structs and queries
use super::Result;
use crate::db::schema::rooms;
use crate::db::users::User;
use crate::db::DbInterface;
use crate::diesel::RunQueryDsl;
use diesel::{ExpressionMethods, QueryDsl, QueryResult};
use diesel::{Identifiable, Queryable};
use uuid::Uuid;

/// Diesel room struct
///
/// Is used as a result in various queries. Represents a room column
#[derive(Debug, Queryable, Identifiable)]
pub struct Room {
    pub id: i64,
    pub uuid: Uuid,
    pub owner: i64,
    pub password: String,
    pub wait_for_moderator: bool,
    pub listen_only: bool,
}

/// Diesel insertable room struct
///
/// Represents fields that have to be provided on room insertion.
#[derive(Debug, Insertable)]
#[table_name = "rooms"]
pub struct NewRoom {
    pub uuid: Uuid,
    pub owner: i64,
    pub password: String,
    pub wait_for_moderator: bool,
    pub listen_only: bool,
}

/// Diesel room struct for updates
///
/// Is used in update queries. None fields will be ignored on update queries
#[derive(Debug, AsChangeset)]
#[table_name = "rooms"]
pub struct ModifyRoom {
    pub owner: Option<i64>,
    pub password: Option<String>,
    pub wait_for_moderator: Option<bool>,
    pub listen_only: Option<bool>,
}

impl DbInterface {
    pub fn get_owned_rooms(&self, user: &User) -> Result<Vec<Room>> {
        let con = self.get_con()?;

        let rooms_result: QueryResult<Vec<Room>> = rooms::table
            .filter(rooms::columns::owner.eq(user.id))
            .get_results(&con);

        match rooms_result {
            Ok(rooms) => Ok(rooms),
            Err(e) => {
                log::error!("Query error getting owned rooms, {}", e);
                Err(e.into())
            }
        }
    }

    pub fn new_room(&self, room: NewRoom) -> Result<Room> {
        let con = self.get_con()?;

        // a UUID collision will result in an internal server error
        let room_result: QueryResult<Room> = diesel::insert_into(rooms::table)
            .values(room)
            .get_result(&con);

        match room_result {
            Ok(rooms) => Ok(rooms),
            Err(e) => {
                log::error!("Query error creating new room, {}", e);
                Err(e.into())
            }
        }
    }

    pub fn modify_room_by_uuid(&self, room_uuid: &Uuid, room: ModifyRoom) -> Result<Room> {
        let con = self.get_con()?;

        let target = rooms::table.filter(rooms::columns::uuid.eq(&room_uuid));
        let room_result = diesel::update(target).set(&room).get_result(&con);

        match room_result {
            Ok(rooms) => Ok(rooms),
            Err(e) => {
                log::error!("Query error modifying room, {}", e);
                Err(e.into())
            }
        }
    }

    pub fn get_room_by_uuid(&self, room_uuid: &Uuid) -> Result<Option<Room>> {
        let con = self.get_con()?;

        let result: QueryResult<Room> = rooms::table
            .filter(rooms::columns::uuid.eq(room_uuid))
            .get_result(&con);

        match result {
            Ok(user) => Ok(Some(user)),
            Err(diesel::NotFound) => Ok(None),
            Err(e) => {
                log::error!("Query error getting room by uuid, {}", e);
                Err(e.into())
            }
        }
    }
}
