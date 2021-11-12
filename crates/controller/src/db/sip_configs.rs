use super::{DatabaseError, Result};
use crate::db::rooms::RoomId;
use crate::db::schema::sip_configs;
use crate::db::DbInterface;
use crate::diesel::RunQueryDsl;
use diesel::{ExpressionMethods, QueryDsl, QueryResult};
use diesel::{Identifiable, Queryable};
use rand::distributions::Slice;
use rand::{thread_rng, Rng};
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
            if !NUMERIC.contains(&c) {
                errors.add("0", ValidationError::new("Invalid id length"));
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

impl From<SipConfigParams> for NewSipConfig {
    fn from(params: SipConfigParams) -> Self {
        Self {
            room: params.room,
            sip_id: SipId::generate(),
            password: params.password,
            enable_lobby: params.enable_lobby,
        }
    }
}

/// Parameters for creating a new sip config
#[derive(Debug, Clone)]
pub struct SipConfigParams {
    pub room: RoomId,
    pub password: SipPassword,
    pub enable_lobby: bool,
}

impl SipConfigParams {
    /// Create new SipConfigParams
    ///
    /// Generates a random password and disables the SIP lobby
    pub fn generate_new(room_id: RoomId) -> Self {
        Self {
            room: room_id,
            password: SipPassword::generate(),
            enable_lobby: false,
        }
    }
}

/// Diesel struct to modify a SipConfig
#[derive(Debug, AsChangeset)]
#[table_name = "sip_configs"]
pub struct ModifySipConfig {
    pub password: Option<SipPassword>,
    pub enable_lobby: Option<bool>,
}

impl DbInterface {
    /// Create a new sip config from the provided [`SipConfigParams`]
    pub fn new_sip_config(&self, params: SipConfigParams) -> Result<SipConfig> {
        let con = self.get_con()?;

        // Attempt a maximum of 3 times to insert a new sip config when a sip_id collision occurs
        for _ in 0..3 {
            let sip_config_result: QueryResult<SipConfig> = diesel::insert_into(sip_configs::table)
                .values(NewSipConfig::from(params.clone()))
                .get_result(&con);

            match sip_config_result {
                Ok(sip_config) => return Ok(sip_config),
                Err(error) => {
                    let mut retry = false;

                    // Check if the error comes from a sip_id collision
                    if let diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::UniqueViolation,
                        info,
                    ) = &error
                    {
                        if let Some(constraint) = info.constraint_name() {
                            if constraint == "sip_configs_sip_id_key" {
                                retry = true
                            }
                        }
                    }

                    if !retry {
                        return Err(error.into());
                    }
                }
            }
        }

        Err(DatabaseError::Error(
            "Unable to insert sip config after 3 attempts with sip_id collision".into(),
        ))
    }

    /// Get the sip config for the specified room
    ///
    /// Returns Ok(None) when the room has no sip config set.
    pub fn get_sip_config(&self, room_id: RoomId) -> Result<Option<SipConfig>> {
        let con = self.get_con()?;

        let config_result = sip_configs::table
            .filter(sip_configs::columns::room.eq(&room_id))
            .get_result(&con);

        match config_result {
            Ok(sip_config) => Ok(Some(sip_config)),
            Err(e) => {
                if let diesel::result::Error::NotFound = e {
                    return Ok(None);
                }

                log::error!("Query error selecting sip config by room_id, {}", e);
                Err(e.into())
            }
        }
    }

    /// Get the sip config for the specified sip_id
    ///
    /// Returns Ok(None) when the no sip config was found.
    pub fn get_sip_config_by_sip_id(&self, sip_id: SipId) -> Result<Option<SipConfig>> {
        let con = self.get_con()?;

        let config_result = sip_configs::table
            .filter(sip_configs::columns::sip_id.eq(&sip_id))
            .get_result(&con);

        match config_result {
            Ok(sip_config) => Ok(Some(sip_config)),
            Err(e) => {
                if let diesel::result::Error::NotFound = e {
                    return Ok(None);
                }

                log::error!("Query error selecting sip config by sip_id, {}", e);
                Err(e.into())
            }
        }
    }

    /// Modify the sip config for the specified room
    ///
    /// Returns Ok(None) when the room has no sip config set.
    pub fn modify_sip_config(
        &self,
        room_id: RoomId,
        config: &ModifySipConfig,
    ) -> Result<Option<SipConfig>> {
        let con = self.get_con()?;

        let target = sip_configs::table.filter(sip_configs::columns::room.eq(&room_id));
        let config_result = diesel::update(target).set(config).get_result(&con);

        match config_result {
            Ok(sip_config) => Ok(Some(sip_config)),
            Err(e) => {
                if let diesel::result::Error::NotFound = e {
                    return Ok(None);
                }

                log::error!("Query error modifying sip config by room_id, {}", e);
                Err(e.into())
            }
        }
    }

    /// Delete the sip config for the specified room
    ///
    /// Returns Ok(None) when the room had no sip config set.
    pub fn delete_sip_config(&self, room_id: RoomId) -> Result<Option<SipConfig>> {
        let con = self.get_con()?;

        let target = sip_configs::table.filter(sip_configs::columns::room.eq(&room_id));
        let config_result = diesel::delete(target).get_result(&con);

        match config_result {
            Ok(sip_config) => Ok(Some(sip_config)),
            Err(e) => {
                if let diesel::result::Error::NotFound = e {
                    return Ok(None);
                }

                log::error!("Query error deleting sip config by room_id, {}", e);
                Err(e.into())
            }
        }
    }
}
