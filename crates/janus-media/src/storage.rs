use super::State;
use anyhow::{Context, Result};
use controller::prelude::*;
use controller_shared::ParticipantId;
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room}:participant={participant}:namespace=media:state
#[ignore_extra_doc_attributes]
/// Data related to a module inside a participant
// TODO can this be removed?
struct MediaState {
    room: SignalingRoomId,
    participant: ParticipantId,
}

impl_to_redis_args!(MediaState);

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn get_state(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<Option<State>> {
    let json: Option<Vec<u8>> = redis_conn
        .get(MediaState { room, participant })
        .await
        .context("Failed to get media state")?;

    if let Some(json) = json {
        serde_json::from_slice(&json).context("Failed to convert json to media state")
    } else {
        Ok(None)
    }
}

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn set_state(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
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

#[tracing::instrument(level = "debug", skip(redis_conn))]
pub async fn del_state(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<()> {
    redis_conn
        .del(MediaState { room, participant })
        .await
        .context("Failed to delete media state")
}
