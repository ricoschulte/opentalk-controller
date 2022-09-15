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
    #[derive(Copy)] RoomId(uuid::Uuid) => diesel::sql_types::Uuid, "/rooms/",
    #[derive(Copy)] SerialRoomId(i64) => diesel::sql_types::BigInt
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
    pub password: Option<String>,
    pub waiting_room: bool,
}

impl Room {
    /// Select a room using the given id
    #[tracing::instrument(err, skip_all)]
    pub fn get(conn: &mut DbConnection, id: RoomId) -> Result<Self> {
        let query = rooms::table.filter(rooms::id.eq(id));

        let room: Room = query.get_result(conn)?;

        Ok(room)
    }

    /// Select a room and the creator using the given room id
    #[tracing::instrument(err, skip_all)]
    pub fn get_with_user(conn: &mut DbConnection, id: RoomId) -> Result<(Self, User)> {
        let query = rooms::table
            .filter(rooms::id.eq(id))
            .inner_join(users::table);

        let result: (Room, User) = query.get_result(conn)?;

        Ok(result)
    }

    /// Select all rooms joined with their creator
    #[tracing::instrument(err, skip_all)]
    pub fn get_all_with_creator(conn: &mut DbConnection) -> Result<Vec<(Room, User)>> {
        let query = rooms::table
            .order_by(rooms::id.desc())
            .inner_join(users::table);

        let room_with_creator = query.load::<(Room, User)>(conn)?;

        Ok(room_with_creator)
    }

    /// Select all rooms paginated
    #[tracing::instrument(err, skip_all)]
    pub fn get_all_with_creator_paginated(
        conn: &mut DbConnection,
        limit: i64,
        page: i64,
    ) -> Result<(Vec<(Room, User)>, i64)> {
        let query = rooms::table
            .inner_join(users::table)
            .select((rooms::all_columns, users::all_columns))
            .order_by(rooms::id.desc())
            .paginate_by(limit, page);

        let rooms_with_total = query.load_and_count(conn)?;

        Ok(rooms_with_total)
    }

    /// Select all rooms filtered by ids
    #[tracing::instrument(err, skip_all)]
    pub fn get_by_ids_with_creator_paginated(
        conn: &mut DbConnection,
        ids: &[RoomId],
        limit: i64,
        page: i64,
    ) -> Result<(Vec<(Room, User)>, i64)> {
        let query = rooms::table
            .inner_join(users::table)
            .select((rooms::all_columns, users::all_columns))
            .filter(rooms::id.eq_any(ids))
            .order_by(rooms::id.desc())
            .paginate_by(limit, page);

        let rooms_with_total = query.load_and_count(conn)?;

        Ok(rooms_with_total)
    }

    /// Delete a room using the given id
    #[tracing::instrument(err, skip_all)]
    pub fn delete_by_id(conn: &mut DbConnection, room_id: RoomId) -> Result<()> {
        let query = diesel::delete(rooms::table.filter(rooms::id.eq(room_id)));

        query.execute(conn)?;

        Ok(())
    }

    /// Delete the room from the database
    pub fn delete(self, conn: &mut DbConnection) -> Result<()> {
        Self::delete_by_id(conn, self.id)
    }
}

/// Diesel insertable room struct
///
/// Represents fields that have to be provided on room insertion.
#[derive(Debug, Insertable)]
#[diesel(table_name = rooms)]
pub struct NewRoom {
    pub created_by: UserId,
    pub password: Option<String>,
    pub waiting_room: bool,
}

impl NewRoom {
    #[tracing::instrument(err, skip_all)]
    pub fn insert(self, conn: &mut DbConnection) -> Result<Room> {
        let room = self.insert_into(rooms::table).get_result(conn)?;

        Ok(room)
    }
}

/// Diesel room struct for updates
///
/// Is used in update queries. None fields will be ignored on update queries
#[derive(Debug, AsChangeset)]
#[diesel(table_name = rooms)]
pub struct UpdateRoom {
    pub password: Option<Option<String>>,
    pub waiting_room: Option<bool>,
}

impl UpdateRoom {
    #[tracing::instrument(err, skip_all)]
    pub fn apply(self, conn: &mut DbConnection, room_id: RoomId) -> Result<Room> {
        let target = rooms::table.filter(rooms::id.eq(&room_id));
        let room = diesel::update(target).set(self).get_result(conn)?;

        Ok(room)
    }
}
