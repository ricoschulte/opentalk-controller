use super::State;
use anyhow::{Context, Result};
use controller::db::rooms::RoomId;
use controller::prelude::*;
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room}:participant={participant}:namespace=media:state
#[ignore_extra_doc_attributes]
/// Data related to a module inside a participant
// TODO can this be removed?
struct MediaState {
    room: RoomId,
    participant: ParticipantId,
}

impl_to_redis_args!(MediaState);

pub async fn get_state(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    participant: ParticipantId,
) -> Result<State> {
    let json: Vec<u8> = redis_conn
        .get(MediaState { room, participant })
        .await
        .context("Failed to get media state")?;

    serde_json::from_slice(&json).context("Failed to convert json to media state")
}

pub async fn set_state(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    participant: ParticipantId,
    state: &State,
) -> Result<()> {
    let json = serde_json::to_vec(&state).context("Failed to convert media state to json")?;

    redis_conn
        .set(MediaState { room, participant }, json)
        .await
        .context("Failed to get media state")?;

    Ok(())
}

pub async fn del_state(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    participant: ParticipantId,
) -> Result<()> {
    redis_conn
        .del(MediaState { room, participant })
        .await
        .context("Failed to delete media state")
}
