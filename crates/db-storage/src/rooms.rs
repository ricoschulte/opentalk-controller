//! Contains the room specific database structs and queries
use crate::diesel::RunQueryDsl;
use crate::schema::rooms;
use crate::schema::users;
use crate::users::{User, UserId};
use chrono::{DateTime, Utc};
use database::DbConnection;
use database::{Paginate, Result};
use diesel::prelude::*;
use diesel::{ExpressionMethods, QueryDsl};
use diesel::{Identifiable, Queryable};

diesel_newtype! {
    #[derive(Copy)] RoomId(uuid::Uuid) => diesel::sql_types::Uuid, "diesel::sql_types::Uuid", "/rooms/",
    #[derive(Copy)] SerialRoomId(i64) => diesel::sql_types::BigInt, "diesel::sql_types::BigInt"
}

/// Diesel room struct
///
/// Is used as a result in various queries. Represents a room column
#[derive(Debug, Clone, Queryable, Identifiable)]
pub struct Room {
    pub id: RoomId,
    pub id_serial: SerialRoomId,
    pub created_by: UserId,
    pub created_at: DateTime<Utc>,
    pub password: String,
    pub wait_for_moderator: bool,
    pub listen_only: bool,
}

impl Room {
    /// Select a room using the given id
    #[tracing::instrument(err, skip_all)]
    pub fn get(conn: &DbConnection, id: RoomId) -> Result<Self> {
        let query = rooms::table.filter(rooms::id.eq(id));

        let room: Room = query.get_result(conn)?;

        Ok(room)
    }

    /// Select all rooms joined with their creator
    #[tracing::instrument(err, skip_all)]
    pub fn get_all_with_creator(conn: &DbConnection) -> Result<Vec<(Room, User)>> {
        let query = rooms::table
            .order_by(rooms::id.desc())
            .inner_join(users::table);

        let room_with_creator = query.load::<(Room, User)>(conn)?;

        Ok(room_with_creator)
    }

    /// Select all rooms paginated
    #[tracing::instrument(err, skip_all)]
    pub fn get_all_paginated(
        conn: &DbConnection,
        limit: i64,
        page: i64,
    ) -> Result<(Vec<Room>, i64)> {
        let query = rooms::table
            .order_by(rooms::id.desc())
            .paginate_by(limit, page);

        let rooms_with_total = query.load_and_count::<Room, _>(conn)?;

        Ok(rooms_with_total)
    }

    /// Select all rooms filtered by ids
    #[tracing::instrument(err, skip_all)]
    pub fn get_by_ids_paginated(
        conn: &DbConnection,
        ids: &[RoomId],
        limit: i64,
        page: i64,
    ) -> Result<(Vec<Room>, i64)> {
        let query = rooms::table
            .filter(rooms::id.eq_any(ids))
            .order_by(rooms::id.desc())
            .paginate_by(limit, page);

        let rooms_with_total = query.load_and_count::<Room, _>(conn)?;

        Ok(rooms_with_total)
    }

    /// Delete a room using the given id
    #[tracing::instrument(err, skip_all)]
    pub fn delete_by_id(conn: &DbConnection, room_id: RoomId) -> Result<()> {
        let query = diesel::delete(rooms::table.filter(rooms::id.eq(room_id)));

        query.execute(conn)?;

        Ok(())
    }

    /// Delete the room from the database
    pub fn delete(self, conn: &DbConnection) -> Result<()> {
        Self::delete_by_id(conn, self.id)
    }
}

/// Diesel insertable room struct
///
/// Represents fields that have to be provided on room insertion.
#[derive(Debug, Insertable)]
#[table_name = "rooms"]
pub struct NewRoom {
    pub created_by: UserId,
    pub password: String,
    pub wait_for_moderator: bool,
    pub listen_only: bool,
}

impl NewRoom {
    #[tracing::instrument(err, skip_all)]
    pub fn insert(self, conn: &DbConnection) -> Result<Room> {
        let room = self.insert_into(rooms::table).get_result(conn)?;

        Ok(room)
    }
}

/// Diesel room struct for updates
///
/// Is used in update queries. None fields will be ignored on update queries
#[derive(Debug, AsChangeset)]
#[table_name = "rooms"]
pub struct UpdateRoom {
    pub password: Option<String>,
    pub wait_for_moderator: Option<bool>,
    pub listen_only: Option<bool>,
}

impl UpdateRoom {
    #[tracing::instrument(err, skip_all)]
    pub fn apply(self, conn: &DbConnection, room_id: RoomId) -> Result<Room> {
        let target = rooms::table.filter(rooms::id.eq(&room_id));
        let room = diesel::update(target).set(self).get_result(conn)?;

        Ok(room)
    }
}
