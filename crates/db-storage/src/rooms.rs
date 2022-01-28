//! Contains the room specific database structs and queries
use crate::diesel::RunQueryDsl;
use crate::schema::rooms;
use crate::schema::users;
use crate::users::SerialUserId;
use crate::users::User;
use database::{DbInterface, Paginate, Result};
use diesel::dsl::any;
use diesel::{Connection, ExpressionMethods, QueryDsl, QueryResult};
use diesel::{Identifiable, Queryable};

diesel_newtype!(#[derive(Copy)] RoomId(uuid::Uuid) => diesel::sql_types::Uuid, "diesel::sql_types::Uuid", "/rooms/");

/// Diesel room struct
///
/// Is used as a result in various queries. Represents a room column
#[derive(Debug, Clone, Queryable, Identifiable)]
pub struct Room {
    pub id: i64,
    pub uuid: RoomId,
    pub owner: SerialUserId,
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
    pub uuid: RoomId,
    pub owner: SerialUserId,
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
    pub owner: Option<SerialUserId>,
    pub password: Option<String>,
    pub wait_for_moderator: Option<bool>,
    pub listen_only: Option<bool>,
}

pub trait DbRoomsEx: DbInterface {
    #[tracing::instrument(skip(self))]
    fn get_rooms_with_creator(&self) -> Result<Vec<(Room, User)>> {
        let conn = self.get_conn()?;

        let query = rooms::table
            .order_by(rooms::columns::id.desc())
            .inner_join(users::table);

        let query_result = query.load::<(Room, User)>(&conn);

        match query_result {
            Ok(rooms) => Ok(rooms),
            Err(e) => {
                log::error!("Query error getting rooms, {}", e);
                Err(e.into())
            }
        }
    }

    #[tracing::instrument(skip(self))]
    fn get_rooms_paginated(&self, limit: i64, page: i64) -> Result<(Vec<Room>, i64)> {
        let conn = self.get_conn()?;

        let query = rooms::table
            .order_by(rooms::columns::id.desc())
            .paginate_by(limit, page);

        let query_result = query.load_and_count::<Room, _>(&conn);

        match query_result {
            Ok(rooms) => Ok(rooms),
            Err(e) => {
                log::error!("Query error getting rooms, {}", e);
                Err(e.into())
            }
        }
    }

    #[tracing::instrument(skip(self))]
    fn get_rooms_by_ids_paginated(
        &self,
        ids: &[RoomId],
        limit: i64,
        page: i64,
    ) -> Result<(Vec<Room>, i64)> {
        let conn = self.get_conn()?;

        let query = rooms::table
            .filter(rooms::columns::uuid.eq(any(ids)))
            .order_by(rooms::columns::id.desc())
            .paginate_by(limit, page);

        let query_result = query.load_and_count::<Room, _>(&conn);

        match query_result {
            Ok(rooms) => Ok(rooms),
            Err(e) => {
                log::error!("Query error getting owned rooms, {}", e);
                Err(e.into())
            }
        }
    }

    #[tracing::instrument(skip(self, room))]
    fn new_room(&self, room: NewRoom) -> Result<Room> {
        let con = self.get_conn()?;

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

    #[tracing::instrument(skip(self, room_id, room))]
    fn modify_room(&self, room_id: RoomId, room: ModifyRoom) -> Result<Room> {
        let con = self.get_conn()?;

        let target = rooms::table.filter(rooms::columns::uuid.eq(&room_id));
        let room_result = diesel::update(target).set(&room).get_result(&con);

        match room_result {
            Ok(rooms) => Ok(rooms),
            Err(e) => {
                log::error!("Query error modifying room, {}", e);
                Err(e.into())
            }
        }
    }

    #[tracing::instrument(skip(self, room_id))]
    fn get_room(&self, room_id: RoomId) -> Result<Option<Room>> {
        let con = self.get_conn()?;

        let result: QueryResult<Room> = rooms::table
            .filter(rooms::columns::uuid.eq(room_id))
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

    #[tracing::instrument(skip(self, room_id))]
    fn delete_room(&self, room_id: RoomId) -> Result<()> {
        let conn = self.get_conn()?;
        let result = conn.transaction::<_, diesel::result::Error, _>(|| {
            diesel::delete(rooms::table.filter(rooms::columns::uuid.eq(room_id))).execute(&conn)?;

            Ok(())
        });

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                log::error!("Failed to delete room by uuid, {}", e);
                Err(e.into())
            }
        }
    }
}
impl<T: DbInterface> DbRoomsEx for T {}
