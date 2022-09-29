use anyhow::{Context, Result};
use controller::prelude::redis::AsyncCommands;
use controller::prelude::*;
use displaydoc::Display;
use serde::{Deserialize, Serialize};

#[derive(Display)]
/// k3k-signaling:room={room_id}:protocol:init
#[ignore_extra_doc_attributes]
///
/// Stores the [`InitState`] of this room.
pub(super) struct InitKey {
    pub(super) room_id: SignalingRoomId,
}

impl_to_redis_args!(InitKey);

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InitState {
    Initializing,
    Initialized,
}

impl_to_redis_args_se!(InitState);
impl_from_redis_value_de!(InitState);

/// Attempts to set the room state to [`InitState::Initializing`] with a SETNX command.
///
/// If the key already holds a value, the current key gets returned without changing the state.
///
/// Behaves like a SETNX-GET redis command.
///
/// When the key was empty and the `Initializing` state was set, Ok(None) will be returned.
#[tracing::instrument(name = "protocol_try_start_init", skip(redis_conn))]
pub(crate) async fn try_start_init(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
) -> Result<Option<InitState>> {
    let affected_entries: i64 = redis_conn
        .set_nx(InitKey { room_id }, InitState::Initializing)
        .await
        .context("Failed to set protocol init state")?;

    if affected_entries == 1 {
        Ok(None)
    } else {
        let state: InitState = redis_conn
            .get(InitKey { room_id })
            .await
            .context("Failed to get protocol init state")?;

        Ok(Some(state))
    }

    // FIXME: use this when redis 7.0 is released
    // redis::cmd("SET")
    //     .arg(InitKey { room_id })
    //     .arg(InitState::Initializing)
    //     .arg("NX")
    //     .arg("GET")
    //     .query_async::<_, Option<InitState>>(redis_conn)
    //     .await
    //     .context("Failed to set protocol init state")
}

/// Sets the room state to [`InitState::Initialized`]
#[tracing::instrument(name = "protocol_set_initialized", skip(redis_conn))]
pub(crate) async fn set_initialized(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
) -> Result<()> {
    redis_conn
        .set(InitKey { room_id }, InitState::Initialized)
        .await
        .context("Failed to set protocol init state to `Initialized`")
}

#[tracing::instrument(name = "get_protocol_init_state", skip(redis_conn))]
pub(crate) async fn get(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
) -> Result<Option<InitState>> {
    redis_conn
        .get(InitKey { room_id })
        .await
        .context("Failed to get protocol init state")
}

#[tracing::instrument(name = "delete_protocol_init_state", skip(redis_conn))]
pub(crate) async fn del(redis_conn: &mut RedisConnection, room_id: SignalingRoomId) -> Result<()> {
    redis_conn
        .del::<_, i64>(InitKey { room_id })
        .await
        .context("Failed to delete protocol init key")?;

    Ok(())
}
