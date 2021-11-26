use crate::api::v1::DefaultApiError;
use crate::db;
use actix_web::web::{Data, Json, Path, ReqData};
use actix_web::{delete, get, put, HttpResponse};
use database::Db;
use db_storage::rooms::{DbRoomsEx, RoomId};
use db_storage::sip_configs::DbSipConfigsEx;
use db_storage::sip_configs::{SipConfigParams, SipId, SipPassword};
use db_storage::users::User;
use serde::{Deserialize, Serialize};
use validator::{Validate, ValidationError};

/// The sip config returned by the API endpoints
#[derive(Debug, Clone, Serialize)]
pub struct SipConfig {
    pub room: RoomId,
    pub sip_id: SipId,
    pub password: SipPassword,
    pub lobby: bool,
}

/// API request parameters to create or modify a sip config.
#[derive(Debug, Validate, Deserialize)]
#[validate(schema(function = "disallow_empty"))]
pub struct PutSipConfig {
    #[validate]
    pub password: Option<SipPassword>,
    pub lobby: Option<bool>,
}

fn disallow_empty(modify_room: &PutSipConfig) -> Result<(), ValidationError> {
    let PutSipConfig { password, lobby } = modify_room;

    if password.is_none() && lobby.is_none() {
        Err(ValidationError::new("ModifySipConfig has no set fields"))
    } else {
        Ok(())
    }
}

/// API Endpoint *GET /rooms/{room_id}/sip*
///
/// Get the sip config for the specified room.
#[get("/rooms/{room_uuid}/sip")]
pub async fn get(
    db_ctx: Data<Db>,
    current_user: ReqData<User>,
    room_id: Path<RoomId>,
) -> Result<Json<SipConfig>, DefaultApiError> {
    let room_id = room_id.into_inner();

    let sip_config = crate::block(move || {
        let room = db_ctx.get_room(room_id)?.ok_or(DefaultApiError::NotFound)?;

        if room.owner != current_user.id {
            return Err(DefaultApiError::InsufficientPermission);
        }

        let db_sip_config = db_ctx
            .get_sip_config(room.uuid)?
            .ok_or(DefaultApiError::NotFound)?;

        Ok(SipConfig {
            room: room.uuid,
            sip_id: db_sip_config.sip_id,
            password: db_sip_config.password,
            lobby: db_sip_config.lobby,
        })
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on GET /rooms{{room_id}}/sip - {}", e);
        DefaultApiError::Internal
    })??;

    Ok(Json(sip_config))
}

/// API Endpoint *PUT /rooms/{room_id}/sip*
///
/// Modifies a sip config with the provided [`PutSipConfig`]. A new sip config is created
/// when no config was set.
///
/// Returns the new modified sip config.
#[put("/rooms/{room_uuid}/sip")]
pub async fn put(
    db_ctx: Data<Db>,
    current_user: ReqData<User>,
    room_id: Path<RoomId>,
    modify_sip_config: Json<PutSipConfig>,
) -> Result<HttpResponse, DefaultApiError> {
    let room_id = room_id.into_inner();
    let modify_sip_config = modify_sip_config.into_inner();

    if let Err(e) = modify_sip_config.validate() {
        log::warn!("API modify room validation error {}", e);
        return Err(DefaultApiError::ValidationFailed);
    }

    let modify_sip_config = db::sip_configs::ModifySipConfig {
        password: modify_sip_config.password,
        enable_lobby: modify_sip_config.lobby,
    };

    let (sip_config, newly_created) = crate::block(move || {
        // Get the requested room
        let room = db_ctx.get_room(room_id)?.ok_or(DefaultApiError::NotFound)?;

        if room.owner != current_user.id {
            return Err(DefaultApiError::InsufficientPermission);
        }

        // Try to modify the sip config before creating a new one
        if let Some(db_sip_config) = db_ctx.modify_sip_config(room.uuid, &modify_sip_config)? {
            let sip_config = SipConfig {
                room: room.uuid,
                sip_id: db_sip_config.sip_id,
                password: db_sip_config.password,
                lobby: db_sip_config.lobby,
            };

            Ok((sip_config, false))
        } else {
            // Create a new sip config
            let sip_params = SipConfigParams {
                room: room.uuid,
                password: modify_sip_config
                    .password
                    .unwrap_or_else(SipPassword::generate),
                enable_lobby: modify_sip_config.enable_lobby.unwrap_or_default(),
            };

            let db_sip_config = db_ctx.new_sip_config(sip_params)?;

            let sip_config = SipConfig {
                room: room.uuid,
                sip_id: db_sip_config.sip_id,
                password: db_sip_config.password,
                lobby: db_sip_config.lobby,
            };

            Ok((sip_config, true))
        }
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on PUT /rooms{{room_id}}/sip - {}", e);
        DefaultApiError::Internal
    })??;

    let mut response = if newly_created {
        HttpResponse::Created()
    } else {
        HttpResponse::Ok()
    };

    Ok(response.json(sip_config))
}

/// API Endpoint *DELETE /rooms/{room_id}/sip*
///
/// Deletes the sip config of the provided room.
#[delete("/rooms/{room_uuid}/sip")]
pub async fn delete(
    db_ctx: Data<Db>,
    current_user: ReqData<User>,
    room_id: Path<RoomId>,
) -> Result<HttpResponse, DefaultApiError> {
    let room_id = room_id.into_inner();

    crate::block(move || {
        // Get the requested room
        let room = db_ctx.get_room(room_id)?.ok_or(DefaultApiError::NotFound)?;

        if room.owner != current_user.id {
            return Err(DefaultApiError::InsufficientPermission);
        }

        db_ctx
            .delete_sip_config(room.uuid)?
            .ok_or(DefaultApiError::NotFound)?;

        Ok(())
    })
    .await
    .map_err(|e| {
        log::error!("BlockingError on PUT /rooms{{room_id}}/sip - {}", e);
        DefaultApiError::Internal
    })??;

    Ok(HttpResponse::NoContent().finish())
}
