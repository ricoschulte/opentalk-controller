//! Single value entry which holds the participant id of the current speaker.
//!
//! If not set, then there is currently no active speaker.
use anyhow::{Context, Result};
use controller::prelude::*;
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room}:automod:speaker
#[ignore_extra_doc_attributes]
/// Typed key to the automod's active speaker
pub struct RoomAutoModSpeaker {
    room: SignalingRoomId,
}

impl_to_redis_args!(RoomAutoModSpeaker);

/// Get the current speaker. Returns [`None`] if there is no active speaker.
#[tracing::instrument(name = "get_speaker", level = "debug", skip(redis_conn))]
pub async fn get(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
) -> Result<Option<ParticipantId>> {
    redis_conn
        .get(RoomAutoModSpeaker { room })
        .await
        .context("Failed to set active speaker")
}

/// Sets the new current speaker and returns the old one if it was set
#[tracing::instrument(name = "set_speaker", level = "debug", skip(redis_conn))]
pub async fn set(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<Option<ParticipantId>> {
    redis::cmd("SET")
        .arg(RoomAutoModSpeaker { room })
        .arg(participant)
        .arg("GET")
        .query_async(redis_conn)
        .await
        .context("Failed to set active speaker")
}

/// Delete the current speaker and return if there was any speaker.
#[tracing::instrument(name = "del_speaker", level = "debug", skip(redis_conn))]
pub async fn del(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
) -> Result<Option<ParticipantId>> {
    redis::cmd("GETDEL")
        .arg(RoomAutoModSpeaker { room })
        .query_async(redis_conn)
        .await
        .context("Failed to del active speaker")
}
