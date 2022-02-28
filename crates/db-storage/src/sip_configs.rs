use super::rooms::RoomId;
use super::schema::sip_configs;
use crate::diesel::RunQueryDsl;
use database::{DatabaseError, DbConnection, Result};
use diesel::prelude::*;
use diesel::{ExpressionMethods, QueryDsl};
use diesel::{Identifiable, Queryable};
use rand::{distributions::Slice, thread_rng, Rng};
use validator::{Validate, ValidationError, ValidationErrors};

/// The set of numbers used to generate [`SipId`] & [`SipPassword`]
const NUMERIC: [char; 10] = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'];

diesel_newtype! {
    NumericId(String) => diesel::sql_types::Text, "diesel::sql_types::Text" ,
    SipId(NumericId) => diesel::sql_types::Text, "diesel::sql_types::Text",
    SipPassword(NumericId) => diesel::sql_types::Text, "diesel::sql_types::Text"
}

impl NumericId {
    pub fn generate() -> Self {
        let numeric_dist = Slice::new(&NUMERIC).unwrap();

        Self::from(thread_rng().sample_iter(numeric_dist).take(10).collect())
    }
}

impl Validate for NumericId {
    fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();

        if self.inner().len() != 10 {
            errors.add("0", ValidationError::new("Invalid id length"));
            return Err(errors);
        }

        for c in self.inner().chars() {
            if !c.is_ascii_digit() {
                errors.add("0", ValidationError::new("Non numeric character"));
                return Err(errors);
            }
        }

        Ok(())
    }
}

impl SipId {
    pub fn generate() -> Self {
        Self::from(NumericId::generate())
    }
}

impl Validate for SipId {
    fn validate(&self) -> Result<(), ValidationErrors> {
        self.inner().validate()
    }
}

impl SipPassword {
    pub fn generate() -> Self {
        Self::from(NumericId::generate())
    }
}

impl Validate for SipPassword {
    fn validate(&self) -> Result<(), ValidationErrors> {
        self.inner().validate()
    }
}

/// Diesel SipConfig struct
#[derive(Debug, Clone, Queryable, Identifiable)]
pub struct SipConfig {
    pub id: i64,
    pub room: RoomId,
    pub sip_id: SipId,
    pub password: SipPassword,
    pub lobby: bool,
}

impl SipConfig {
    /// Get the sip config for the specified sip_id
    #[tracing::instrument(err, skip_all)]
    pub fn get(conn: &DbConnection, sip_id: SipId) -> Result<SipConfig> {
        let query = sip_configs::table.filter(sip_configs::sip_id.eq(&sip_id));
        let sip_config = query.get_result(conn)?;

        Ok(sip_config)
    }

    /// Get the sip config for the specified room
    #[tracing::instrument(err, skip_all)]
    pub fn get_by_room(conn: &DbConnection, room_id: RoomId) -> Result<SipConfig> {
        let query = sip_configs::table.filter(sip_configs::room.eq(&room_id));
        let sip_config = query.get_result(conn)?;

        Ok(sip_config)
    }

    /// Delete the sip config for the specified room
    #[tracing::instrument(err, skip_all)]
    pub fn delete_by_room(conn: &DbConnection, room_id: RoomId) -> Result<()> {
        let query = diesel::delete(sip_configs::table.filter(sip_configs::room.eq(&room_id)));

        query.execute(conn)?;

        Ok(())
    }

    pub fn delete(&self, conn: &DbConnection) -> Result<()> {
        Self::delete_by_room(conn, self.room)
    }
}

/// Diesel insertable SipConfig struct
///
/// Represents fields that have to be provided on insertion.
#[derive(Debug, Clone, Insertable)]
#[table_name = "sip_configs"]
pub struct NewSipConfig {
    pub room: RoomId,
    pub sip_id: SipId,
    pub password: SipPassword,
    pub enable_lobby: bool,
}

impl NewSipConfig {
    pub fn new(room_id: RoomId, enable_lobby: bool) -> Self {
        Self {
            room: room_id,
            sip_id: SipId::generate(),
            password: SipPassword::generate(),
            enable_lobby,
        }
    }

    fn re_generate_id(&mut self) {
        self.sip_id = SipId::generate();
    }

    #[tracing::instrument(err, skip_all)]
    pub fn insert(mut self, conn: &DbConnection) -> Result<SipConfig> {
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

        Err(DatabaseError::Custom(format!(
            "Failed to insert new sip config for room {room} 3 times (collision)",
            room = self.room
        )))
    }
}

/// Diesel struct to modify a SipConfig
#[derive(Debug, AsChangeset)]
#[table_name = "sip_configs"]
pub struct UpdateSipConfig {
    pub password: Option<SipPassword>,
    pub enable_lobby: Option<bool>,
}

impl UpdateSipConfig {
    pub fn apply(self, conn: &DbConnection, room_id: RoomId) -> Result<SipConfig> {
        let query =
            diesel::update(sip_configs::table.filter(sip_configs::room.eq(&room_id))).set(self);

        let config = query.get_result(conn)?;

        Ok(config)
    }
}
