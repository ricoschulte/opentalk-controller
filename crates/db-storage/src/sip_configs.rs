// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use super::schema::sip_configs;
use crate::diesel::RunQueryDsl;
use database::{DatabaseError, DbConnection, Result};
use diesel::prelude::*;
use diesel::{ExpressionMethods, QueryDsl};
use diesel::{Identifiable, Queryable};
use types::core::{CallInId, CallInPassword, RoomId};

/// Diesel SipConfig struct
#[derive(Debug, Clone, Queryable, Identifiable)]
pub struct SipConfig {
    pub id: i64,
    pub room: RoomId,
    pub sip_id: CallInId,
    pub password: CallInPassword,
    pub lobby: bool,
}

impl SipConfig {
    /// Get the sip config for the specified sip_id
    #[tracing::instrument(err, skip_all)]
    pub fn get(conn: &mut DbConnection, sip_id: CallInId) -> Result<Option<SipConfig>> {
        let query = sip_configs::table.filter(sip_configs::sip_id.eq(&sip_id));
        let sip_config = query.get_result(conn).optional()?;

        Ok(sip_config)
    }

    /// Get the sip config for the specified room
    #[tracing::instrument(err, skip_all)]
    pub fn get_by_room(conn: &mut DbConnection, room_id: RoomId) -> Result<SipConfig> {
        let query = sip_configs::table.filter(sip_configs::room.eq(&room_id));
        let sip_config = query.get_result(conn)?;

        Ok(sip_config)
    }

    /// Delete the sip config for the specified room
    #[tracing::instrument(err, skip_all)]
    pub fn delete_by_room(conn: &mut DbConnection, room_id: RoomId) -> Result<()> {
        let query = diesel::delete(sip_configs::table.filter(sip_configs::room.eq(&room_id)));

        query.execute(conn)?;

        Ok(())
    }

    pub fn delete(&self, conn: &mut DbConnection) -> Result<()> {
        Self::delete_by_room(conn, self.room)
    }
}

/// Diesel insertable SipConfig struct
///
/// Represents fields that have to be provided on insertion.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = sip_configs)]
pub struct NewSipConfig {
    pub room: RoomId,
    pub sip_id: CallInId,
    pub password: CallInPassword,
    pub enable_lobby: bool,
}

impl NewSipConfig {
    pub fn new(room_id: RoomId, enable_lobby: bool) -> Self {
        Self {
            room: room_id,
            sip_id: CallInId::generate(),
            password: CallInPassword::generate(),
            enable_lobby,
        }
    }

    fn re_generate_id(&mut self) {
        self.sip_id = CallInId::generate();
    }

    #[tracing::instrument(err, skip_all)]
    pub fn insert(mut self, conn: &mut DbConnection) -> Result<SipConfig> {
        for _ in 0..3 {
            let query = self.clone().insert_into(sip_configs::table);

            let config = match query.get_result(conn) {
                Ok(config) => config,
                Err(diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::UniqueViolation,
                    _,
                )) => {
                    self.re_generate_id();
                    continue;
                }
                Err(e) => return Err(e.into()),
            };

            return Ok(config);
        }

        Err(DatabaseError::custom(format!(
            "Failed to insert new sip config for room {room} 3 times (collision)",
            room = self.room
        )))
    }
}

/// Diesel struct to modify a SipConfig
#[derive(Debug, AsChangeset)]
#[diesel(table_name = sip_configs)]
pub struct UpdateSipConfig {
    pub password: Option<CallInPassword>,
    pub enable_lobby: Option<bool>,
}

impl UpdateSipConfig {
    pub fn apply(self, conn: &mut DbConnection, room_id: RoomId) -> Result<Option<SipConfig>> {
        let query =
            diesel::update(sip_configs::table.filter(sip_configs::room.eq(&room_id))).set(self);

        let config = query.get_result(conn).optional()?;

        Ok(config)
    }
}
